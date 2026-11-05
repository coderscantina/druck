//! The spike's gate, measured with everything running. Off unless built with the `measure` feature, or as a debug
//! build, and run with `DRUCK_GATE=1`. Any other value only opens the window
//! as a measurement does, floating without focus, for screenshots.
//!
//! Types into the middle of the open document on a timer, through the same input path as the keyboard, while pages
//! refresh in the background, then prints typing latency, pause to ready pages, how fast Publish opens, theme edit to updated
//! pages, and memory over many refreshes, and quits. `DRUCK_GATE_THEME_EDITS=0` skips the theme edits, for a read-only
//! theme. `DRUCK_SOAK_MEMORY=1` only reports, for a run driven from outside.
//!
//! `DRUCK_GATE_SLIP=100` instead edits the book through the proof slip that many times, in Publish, and checks that no
//! text was lost: what the chapters hold is compared byte for byte, in memory and on disk, with what was asked for.
//!
//! Times run from when a key was due, so they include any wait for the main thread, to the end of the first paint
//! showing it. Presenting takes one more frame at most. `DRUCK_GATE_REFRESHES` sets the memory run's length.

use std::cell::Cell;
use std::time::{Duration, Instant};

use gpui::{AnyWindowHandle, App, AsyncApp, Entity, EntityInputHandler};

use crate::editor::EditorView;
use crate::inspector::InspectorView;
use crate::preview::{PreviewView, memory};
use crate::workspace::{Mode, Workspace};

/// Bursts of typing at about 130 words a minute, with pauses that let a refresh start.
const BURSTS: usize = 60;
const BURST_KEYS: usize = 6;
const KEY_INTERVAL: Duration = Duration::from_millis(90);
/// Longer than the shell's pause, so the next burst lands while the layout runs.
const BETWEEN_BURSTS: Duration = Duration::from_millis(300);
const PAUSE_SAMPLES: usize = 30;
const THEME_SAMPLES: usize = 12;
const OPEN_SAMPLES: usize = 12;
const REFRESHES: usize = 200;
const POLL: Duration = Duration::from_millis(1);
/// Longer than any layout should take, so a stuck run reports rather than hangs.
const GIVE_UP: Duration = Duration::from_secs(10);
const MARK: &str = "QQQ";

struct Gate {
    workspace: Entity<Workspace>,
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    window: AnyWindowHandle,
    /// Keys typed by [`Gate::letter`].
    letters: Cell<usize>,
}

/// A variable that changes what the app does, for screenshots and measurements. A debug build or a build with the
/// `measure` feature reads them. Any other build, so any app that ships, has no hidden behaviour.
pub fn var(name: &str) -> Option<String> {
    if cfg!(any(debug_assertions, feature = "measure")) {
        std::env::var(name).ok()
    } else {
        None
    }
}

pub fn start(
    workspace: Entity<Workspace>,
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    inspector: Entity<InspectorView>,
    window: AnyWindowHandle,
    cx: &mut App,
) {
    if var("DRUCK_SOAK_MEMORY").is_some() {
        soak(editor, preview, cx);
        return;
    }
    if var("DRUCK_GATE").as_deref() != Some("1") {
        return;
    }
    let gate = Gate {
        workspace,
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

/// For a long run driven from outside: prints the memory each minute and, for each new pages, how long after the last
/// change of the text they were painted. `DRUCK_SOAK_MEMORY=1` turns it on.
fn soak(editor: Entity<EditorView>, preview: Entity<PreviewView>, cx: &mut App) {
    let started = Instant::now();
    cx.spawn(async move |cx| {
        let mut printed = Instant::now();
        let mut revision = 0;
        let mut changed: Option<Instant> = None;
        loop {
            cx.background_executor().timer(Duration::from_millis(10)).await;
            let now = editor.read_with(cx, |editor, _| editor.revision());
            if now != revision {
                revision = now;
                changed = Some(Instant::now());
            }
            let painted = preview.read_with(cx, |preview, _| preview.ready_at());
            if let (Some(from), Some(painted)) = (changed, painted)
                && painted > from
            {
                eprintln!(
                    "soak: pages {} ms after the last change, at {} s",
                    painted.duration_since(from).as_millis(),
                    started.elapsed().as_secs()
                );
                changed = None;
            }
            if printed.elapsed() >= Duration::from_secs(60) {
                printed = Instant::now();
                eprintln!("soak: {} s; {}", started.elapsed().as_secs(), memory());
            }
        }
    })
    .detach();
}

impl Gate {
    async fn run(&self, cx: &mut AsyncApp) {
        self.wait_for_pages(cx).await;
        eprintln!("gate: first pages; {}", memory());
        if let Some(count) = var("DRUCK_GATE_SLIP").and_then(|count| count.parse().ok()) {
            self.slip_edits(count, cx).await;
            return;
        }
        self.place_caret(cx).await;
        if var("DRUCK_GATE_THEME_ONLY").is_none() {
            self.typing(cx).await;
            self.pause_to_pages(cx).await;
            self.entering_publish(cx).await;
        }
        if var("DRUCK_GATE_THEME_EDITS").as_deref() != Some("0") {
            self.theme_edits(cx).await;
        }
        self.memory(cx).await;
    }

    async fn sleep(&self, duration: Duration, cx: &mut AsyncApp) {
        cx.background_executor().timer(duration).await;
    }

    /// Waits until the pages of the text as it stands are ready to paint, and returns when that was. The text may still
    /// be waiting out its pause, so this asks about the text and not the last request.
    async fn wait_for_pages(&self, cx: &mut AsyncApp) -> Instant {
        let start = Instant::now();
        loop {
            let revision = self.editor.read_with(cx, |editor, _| editor.revision());
            let ready = self.preview.read_with(cx, |preview, _| {
                preview.shows(revision).then(|| preview.ready_at()).flatten()
            });
            if let Some(ready) = ready {
                return ready;
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
            quiet.push(self.wait_for_pages(cx).await.saturating_duration_since(last));
            self.settle(cx).await;
        }
        report("last key to ready pages, nothing running at the key", quiet);

        // The last key lands just after a layout started, whose request then cancels that layout.
        let mut busy = Vec::new();
        for _ in 0..PAUSE_SAMPLES {
            self.letter(cx);
            while !self.preview.read_with(cx, |preview, _| preview.is_laying_out()) {
                self.sleep(POLL, cx).await;
            }
            self.letter(cx);
            let last = Instant::now();
            busy.push(self.wait_for_pages(cx).await.saturating_duration_since(last));
            self.settle(cx).await;
        }
        report("last key to ready pages, key just after a layout started", busy);
    }

    /// From a pause in Write, how long until Publish's first frame with every visible page exact.
    async fn entering_publish(&self, cx: &mut AsyncApp) {
        let mut samples = Vec::new();
        for _ in 0..OPEN_SAMPLES {
            self.letter(cx);
            self.settle(cx).await;
            self.sleep(Duration::from_millis(200), cx).await;
            let start = Instant::now();
            self.show(Mode::Publish, cx);
            loop {
                let exact = self.preview.read_with(cx, |preview, _| preview.exact_frame());
                if exact.is_some_and(|frame| frame > start) || start.elapsed() > GIVE_UP {
                    samples.push(start.elapsed());
                    break;
                }
                self.sleep(POLL, cx).await;
            }
            self.show(Mode::Write, cx);
            self.sleep(Duration::from_millis(100), cx).await;
        }
        report("pause in Write to exact pages in Publish", samples);
    }

    /// Opens a slip on a paragraph of a page some way into the book, and returns the page.
    async fn open_slip(&self, page: usize, cx: &mut AsyncApp) -> Option<usize> {
        let (page, block) = self
            .preview
            .read_with(cx, |preview, _| preview.paragraph_on_page(page))?;
        let workspace = self.workspace.clone();
        self.window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.open_slip_at(Some(block), window, cx));
            })
            .ok();
        self.sleep(Duration::from_millis(30), cx).await;
        Some(page)
    }

    /// Types into the open slip through the field's input handler, as an input method or the keyboard does.
    fn slip_key(&self, text: &str, cx: &mut AsyncApp) {
        let preview = self.preview.clone();
        self.window
            .update(cx, |_, window, cx| {
                let input = preview.read(cx).slip().read(cx).input().clone();
                input.update(cx, |input, cx| input.replace_text_in_range(None, text, window, cx));
            })
            .ok();
    }

    fn chapter_texts(&self, cx: &mut AsyncApp) -> Vec<(crate::session::ChapterId, std::path::PathBuf, String)> {
        self.editor.read_with(cx, |editor, _| {
            let session = editor.session();
            session
                .book()
                .iter()
                .filter_map(|id| {
                    let chapter = session.chapter(*id)?;
                    Some((*id, chapter.path().to_owned(), chapter.text().to_owned()))
                })
                .collect()
        })
    }

    async fn slip_edits(&self, count: usize, cx: &mut AsyncApp) {
        self.show(Mode::Publish, cx);
        self.settle(cx).await;
        self.clicks(cx).await;
        let before = self.chapter_texts(cx);
        let mut expected = before.clone();
        let mut to_pages = Vec::new();
        let mut typing = Vec::new();
        let mut lost_opens = 0;
        for edit in 0..count {
            // Across the book, so the edits land in many chapters and on pages of every kind.
            let Some(page) = self.open_slip(edit * 7, cx).await else {
                lost_opens += 1;
                continue;
            };
            let opened = self
                .preview
                .read_with(cx, |preview, cx| preview.slip().read(cx).opened().cloned());
            let Some(opened) = opened else {
                lost_opens += 1;
                continue;
            };
            let word = format!(" qslip{edit}x");
            let at = opened.caret;
            // The caret is where the click was. What is typed goes in at a word's end of the paragraph.
            for letter in word.chars() {
                let started = Instant::now();
                self.slip_key(&letter.to_string(), cx);
                typing.push(started.elapsed());
                self.sleep(Duration::from_millis(12), cx).await;
            }
            let last = Instant::now();
            if let Some((_, _, text)) = expected.iter_mut().find(|(id, ..)| *id == opened.chapter) {
                text.insert_str(opened.range.start + at, &word);
            }
            let ready = self.wait_for_pages(cx).await;
            to_pages.push(ready.saturating_duration_since(last));
            let found = self
                .preview
                .update(cx, |preview, cx| preview.show_text(word.trim_start(), cx));
            if found.is_none() {
                eprintln!("gate: edit {edit} (page {page}) is on no page: {word}");
            }
            self.close_slip(cx);
            self.sleep(Duration::from_millis(40), cx).await;
        }
        report("slip edit, handling a key", typing);
        report("slip edit, last key to ready pages", to_pages);
        eprintln!("gate: slips that did not open: {lost_opens}");
        self.compare(&expected, "after the edits", cx);
        self.moving_paragraph(cx).await;
        self.compare(&expected, "after the page break came away", cx);
        // Autosave runs after a pause.
        self.sleep(Duration::from_secs(4), cx).await;
        let mut on_disk = 0;
        for (_, path, text) in &expected {
            if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
                on_disk += 1;
            } else {
                eprintln!("gate: the file differs from what was asked: {}", path.display());
            }
        }
        eprintln!("gate: chapters on disk as asked: {on_disk} of {}", expected.len());
        self.theme_field(cx).await;
        let (marked, took) = self.preview.update(cx, |preview, cx| preview.mark_again(cx));
        eprintln!("gate: marking {marked} problems on the pages took {took:?}, on the window's thread");
    }

    /// A press and release of the left button, through the window's own event path, at a place in the window.
    fn click(&self, position: gpui::Point<gpui::Pixels>, cx: &mut AsyncApp) {
        self.window
            .update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position,
                        pressed_button: None,
                        modifiers: gpui::Modifiers::default(),
                    }),
                    cx,
                );
                window.dispatch_event(
                    gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                        button: gpui::MouseButton::Left,
                        position,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                        button: gpui::MouseButton::Left,
                        position,
                        modifiers: gpui::Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                );
            })
            .ok();
    }

    /// Clicks as a person would: on a line of a spread, on a contact sheet, and on nothing.
    async fn clicks(&self, cx: &mut AsyncApp) {
        self.preview.update(cx, |preview, cx| preview.show_page(20, cx));
        self.sleep(Duration::from_millis(300), cx).await;
        let spread = self.preview.read_with(cx, |preview, _| preview.point_on_a_line());
        let Some(spread) = spread else {
            eprintln!("gate: no line to click");
            return;
        };
        self.click(spread, cx);
        self.sleep(Duration::from_millis(300), cx).await;
        let opened = self
            .preview
            .read_with(cx, |preview, cx| preview.slip().read(cx).opened().is_some());
        eprintln!("gate: a click on a line opened the slip: {opened}");
        self.preview.update(cx, |preview, cx| preview.zoom_step(-1, cx));
        self.sleep(Duration::from_millis(300), cx).await;
        let contact = self.preview.read_with(cx, |preview, _| preview.is_contact_sheet());
        let middle = self.preview.read_with(cx, |preview, _| preview.middle_of_view());
        self.click(middle, cx);
        self.sleep(Duration::from_millis(300), cx).await;
        let spread_again = self.preview.read_with(cx, |preview, _| !preview.is_contact_sheet());
        eprintln!(
            "gate: zooming out made a contact sheet: {contact}; a click on it went back to a spread: {spread_again}"
        );
        self.close_slip(cx);
        self.sleep(Duration::from_millis(100), cx).await;
    }

    fn close_slip(&self, cx: &mut AsyncApp) {
        let workspace = self.workspace.clone();
        self.window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.close_slip_for_gate(window, cx);
                });
            })
            .ok();
    }

    fn compare(
        &self,
        expected: &[(crate::session::ChapterId, std::path::PathBuf, String)],
        when: &str,
        cx: &mut AsyncApp,
    ) {
        let now = self.chapter_texts(cx);
        let same = now.iter().zip(expected).filter(|(a, b)| a.2 == b.2).count();
        eprintln!("gate: chapters in memory as asked {when}: {same} of {}", expected.len());
        for ((_, path, have), (_, _, wanted)) in now.iter().zip(expected) {
            if have != wanted {
                let at = have.bytes().zip(wanted.bytes()).take_while(|(a, b)| a == b).count();
                eprintln!(
                    "gate: {} differs at byte {at}: {:?} against {:?}",
                    path.display(),
                    have.get(at..(at + 40).min(have.len())),
                    wanted.get(at..(at + 40).min(wanted.len()))
                );
            }
        }
    }

    /// A page break before a paragraph moves it to another page: the slip goes with it. Taking it away restores the bytes.
    async fn moving_paragraph(&self, cx: &mut AsyncApp) {
        let Some(_) = self.open_slip(40, cx).await else {
            eprintln!("gate: no paragraph to move");
            return;
        };
        self.settle(cx).await;
        let from = self.preview.read_with(cx, |preview, _| preview.slip_page());
        let toggle = |gate: &Self, cx: &mut AsyncApp| {
            let workspace = gate.workspace.clone();
            gate.window
                .update(cx, |_, window, cx| {
                    workspace.update(cx, |workspace, cx| workspace.toggle_page_break_for_gate(window, cx));
                })
                .ok();
        };
        toggle(self, cx);
        self.sleep(Duration::from_millis(250), cx).await;
        self.settle(cx).await;
        self.sleep(Duration::from_millis(100), cx).await;
        let (to, shown) = self
            .preview
            .read_with(cx, |preview, _| (preview.slip_page(), preview.slip_in_view()));
        eprintln!("gate: a page break moved the slip's paragraph from page {from:?} to {to:?}, in view: {shown}");
        let has_directive = self.editor.read_with(cx, |editor, _| {
            editor.session().book().iter().any(|id| {
                editor.session().buffer(*id).text().contains(
                    "::: page-break

",
                )
            })
        });
        eprintln!("gate: the directive is in the source: {has_directive}");
        toggle(self, cx);
        self.sleep(Duration::from_millis(250), cx).await;
        self.settle(cx).await;
        self.close_slip(cx);
    }

    /// A style change writes one field of the theme file and nothing else.
    async fn theme_field(&self, cx: &mut AsyncApp) {
        let path = self.inspector.read_with(cx, |inspector, cx| {
            inspector
                .model()
                .read(cx)
                .file()
                .ok()
                .map(|file| file.path().to_owned())
        });
        let Some(path) = path else {
            eprintln!("gate: no theme file to change");
            return;
        };
        let before = std::fs::read_to_string(&path).unwrap_or_default();
        let start = Instant::now();
        self.inspector
            .update(cx, |inspector, cx| inspector.set_body_size("10.7pt", cx));
        while start.elapsed() < GIVE_UP && self.preview.read_with(cx, |preview, _| preview.ready_at()).is_some() {
            self.sleep(POLL, cx).await;
        }
        let ready = self.wait_for_pages(cx).await;
        let after = std::fs::read_to_string(&path).unwrap_or_default();
        let same_start = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
        let same_end = before[same_start..]
            .bytes()
            .rev()
            .zip(after[same_start..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        eprintln!(
            "gate: a style change reached the pages in {:?}, and of the theme file's {} bytes it replaced {:?} with {:?}",
            ready.saturating_duration_since(start),
            before.len(),
            &before[same_start..before.len() - same_end],
            &after[same_start..after.len() - same_end],
        );
    }

    fn show(&self, mode: Mode, cx: &mut AsyncApp) {
        let workspace = self.workspace.clone();
        self.window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.show(mode, window, cx));
            })
            .ok();
    }

    async fn theme_edits(&self, cx: &mut AsyncApp) {
        let mut pages = Vec::new();
        let mut galley = Vec::new();
        let mut blocked = Vec::new();
        for sample in 0..THEME_SAMPLES {
            self.settle(cx).await;
            let generation = self.editor.read_with(cx, |editor, _| editor.style_generation());
            // Differs from the last sample every time, and stays near a real body size.
            let size = format!("{}pt", 10.5 + (sample % 4) as f32 * 0.25 + 0.1);
            let start = Instant::now();
            self.inspector
                .update(cx, |inspector, cx| inspector.set_body_size(&size, cx));
            blocked.push(start.elapsed());
            // The write runs on its own thread, and the pages are requested once it has landed.
            while start.elapsed() < GIVE_UP && self.preview.read_with(cx, |preview, _| preview.ready_at()).is_some() {
                self.sleep(POLL, cx).await;
            }
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
        report("theme edit, window thread busy with the change", blocked);
        report("theme edit to ready pages", pages);
        report("theme edit to painted galley", galley);
    }

    async fn memory(&self, cx: &mut AsyncApp) {
        let refreshes = var("DRUCK_GATE_REFRESHES")
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
