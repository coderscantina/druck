//! The export panel: the named exports of `druck.yaml` with their settings, a run button with progress and cancel, and
//! the finished file to open or reveal. A single file offers a plain PDF and EPUB.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, Focusable, SharedString, Subscription, Window, div, px, svg};

use super::export::{Stage, Target};
use super::manifest::Def;
use super::text::Key;
use super::ui;
use crate::a11y::Control as _;
use crate::inspector::input::{Committed, Submitted, TextInput};
use crate::tokens::{self, Tokens};

pub enum ExportEvent {
    Run(Target),
    Cancel,
    /// Set one key of a named export, or remove it for `None`. The value is the text as typed.
    Set {
        name: String,
        key: &'static str,
        value: Option<String>,
    },
    Add(String),
    Open(PathBuf),
    Reveal(PathBuf),
}

impl EventEmitter<ExportEvent> for ExportPanel {}

/// What the document offers: one file's plain export, or a project's named exports.
#[derive(Debug, Clone, Default)]
pub struct Setup {
    pub single: bool,
    pub defs: Vec<Def>,
    /// Why the exports cannot be read.
    pub problem: Option<String>,
}

/// Where the latest run is.
#[derive(Debug, Clone)]
pub enum RunState {
    Idle,
    Running {
        target: Target,
        stage: Stage,
    },
    Done {
        output: PathBuf,
        parts: usize,
        seconds: f32,
        epub: bool,
    },
    Cancelled,
    Failed,
}

struct Fields {
    theme: Entity<TextInput>,
    output: Entity<TextInput>,
    bleed: Entity<TextInput>,
}

pub struct ExportPanel {
    setup: Setup,
    run: RunState,
    expanded: HashSet<String>,
    fields: HashMap<String, Fields>,
    adding: bool,
    new_name: Entity<TextInput>,
    add_error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl ExportPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let new_name = cx.new(|cx| TextInput::new(window, cx).with_placeholder(crate::t!(Key::ExportName)));
        let subscription = cx.subscribe(&new_name, |panel, _, _: &Submitted, cx| panel.add(cx));
        Self {
            setup: Setup::default(),
            run: RunState::Idle,
            expanded: HashSet::new(),
            fields: HashMap::new(),
            adding: false,
            new_name,
            add_error: None,
            _subscriptions: vec![subscription],
        }
    }

    pub fn set_setup(&mut self, setup: Setup, window: &mut Window, cx: &mut Context<Self>) {
        for def in &setup.defs {
            if !self.fields.contains_key(&def.name) {
                let fields = self.make_fields(&def.name, window, cx);
                self.fields.insert(def.name.clone(), fields);
            }
            let fields = &self.fields[&def.name];
            let values = [
                (&fields.theme, def.theme.clone()),
                (&fields.output, def.output.clone()),
                (&fields.bleed, def.bleed.clone()),
            ];
            for (input, value) in values {
                if input.read(cx).text() != value {
                    input.update(cx, |input, cx| input.set_value(value, true, cx));
                }
            }
        }
        self.setup = setup;
        cx.notify();
    }

    /// Puts the field's hint in the language now set.
    pub fn refresh_text(&mut self, cx: &mut Context<Self>) {
        self.new_name
            .update(cx, |input, cx| input.set_placeholder(crate::t!(Key::ExportName), cx));
    }

    pub fn set_run(&mut self, run: RunState, cx: &mut Context<Self>) {
        self.run = run;
        cx.notify();
    }

    pub fn is_running(&self) -> bool {
        matches!(self.run, RunState::Running { .. })
    }

    fn make_fields(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> Fields {
        let mut make = |key: &'static str, placeholder: &'static str| {
            let input = cx.new(|cx| TextInput::new(window, cx).with_placeholder(placeholder));
            let name = name.to_owned();
            self._subscriptions
                .push(cx.subscribe(&input, move |_, _, Committed(text): &Committed, cx| {
                    let value = Some(text.trim().to_owned()).filter(|text| !text.is_empty());
                    cx.emit(ExportEvent::Set {
                        name: name.clone(),
                        key,
                        value,
                    });
                }));
            input
        };
        Fields {
            theme: make("theme", ""),
            output: make("output", "build/name.pdf"),
            bleed: make("bleed", "3mm"),
        }
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let name = self.new_name.read(cx).text().trim().to_owned();
        self.add_error = if !super::manifest::is_valid_name(&name) {
            Some(crate::t!(Key::ExportBadName))
        } else if self.setup.defs.iter().any(|def| def.name == name) {
            Some(crate::t!(Key::ExportExists))
        } else {
            None
        };
        if self.add_error.is_none() {
            self.adding = false;
            self.expanded.insert(name.clone());
            self.new_name
                .update(cx, |input, cx| input.set_value(String::new(), true, cx));
            cx.emit(ExportEvent::Add(name));
        }
        cx.notify();
    }

    fn progress(&self, stage: Stage, tokens: &Tokens) -> gpui::Div {
        let segments = (0..Stage::COUNT).map(|at| {
            let current = stage as usize;
            let color = if at <= current { tokens.accent } else { tokens.divider };
            div().flex_1().h(px(3.)).rounded(px(1.5)).bg(color)
        });
        div().flex().gap(px(2.)).children(segments)
    }

    fn status(&self, tokens: &Tokens, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let quiet = |text: SharedString| div().text_color(tokens.text_quiet).child(text);
        match &self.run {
            RunState::Idle | RunState::Running { .. } => None,
            RunState::Cancelled => Some(quiet(crate::t!(Key::ExportCancelled).into()).into_any_element()),
            RunState::Failed => Some(
                div()
                    .text_color(tokens.accent)
                    .child(crate::t!(Key::ExportFailedSee))
                    .into_any_element(),
            ),
            RunState::Done {
                output,
                parts,
                seconds,
                epub,
            } => {
                let seconds = format!("{seconds:.1}");
                let line = if *epub {
                    crate::t!(Key::ExportDoneSections, parts = parts, seconds = seconds)
                } else {
                    crate::t!(Key::ExportDonePages, parts = parts, seconds = seconds)
                };
                let (open, reveal) = (output.clone(), output.clone());
                let name = output
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .gap(tokens::SPACE_2)
                        .child(div().truncate().child(name))
                        .child(quiet(line.into()))
                        .child(
                            div()
                                .flex()
                                .gap(tokens::SPACE_2)
                                .child(
                                    ui::button(tokens, "open-file", crate::t!(Key::OpenFile)).on_click(
                                        cx.listener(move |_, _, _, cx| cx.emit(ExportEvent::Open(open.clone()))),
                                    ),
                                )
                                .child(ui::button(tokens, "reveal-file", crate::t!(Key::RevealFile)).on_click(
                                    cx.listener(move |_, _, _, cx| cx.emit(ExportEvent::Reveal(reveal.clone()))),
                                )),
                        )
                        .into_any_element(),
                )
            }
        }
    }

    /// The run button of a target, or its progress and cancel while it runs.
    fn runner(
        &self,
        id: String,
        target: Target,
        label: &'static str,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if let RunState::Running { target: running, stage } = &self.run
            && *running == target
        {
            let text = match stage {
                Stage::Preparing => crate::t!(Key::StagePreparing),
                Stage::Typesetting => crate::t!(Key::StageTypesetting),
                Stage::Writing => crate::t!(Key::StageWriting),
            };
            return div()
                .flex()
                .items_center()
                .gap(tokens::SPACE_2)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(tokens::SPACE_1)
                        .child(
                            div()
                                .truncate()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_quiet)
                                .child(text),
                        )
                        .child(self.progress(*stage, tokens)),
                )
                .child(
                    ui::button(
                        tokens,
                        SharedString::from(format!("cancel-{id}")),
                        crate::t!(Key::ExportCancel),
                    )
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ExportEvent::Cancel))),
                )
                .into_any_element();
        }
        let busy = self.is_running();
        ui::button(tokens, SharedString::from(format!("run-{id}")), label)
            .when(busy, |button| button.opacity(0.4).cursor_default())
            .when(!busy, |button| {
                button.on_click(cx.listener(move |_, _, _, cx| cx.emit(ExportEvent::Run(target.clone()))))
            })
            .into_any_element()
    }

    fn card(&self, def: &Def, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::AnyElement {
        let name = def.name.clone();
        let open = self.expanded.contains(&name);
        let toggle_name = name.clone();
        let mut flags = Vec::new();
        for (on, key) in [
            (def.is_final, Key::FieldFinal),
            (def.draft, Key::FieldDraft),
            (def.crop_marks, Key::FieldCropMarks),
        ] {
            if on {
                flags.push(crate::t!(key));
            }
        }
        let output = if def.output.is_empty() {
            format!("build/{name}.{}", "pdf")
        } else {
            def.output.clone()
        };
        let summary = std::iter::once(output)
            .chain(flags.iter().map(|flag| (*flag).to_owned()))
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        let header = div()
            .id(SharedString::from(format!("card-{name}")))
            .button(tokens, name.clone())
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _, _, cx| {
                if !panel.expanded.remove(&toggle_name) {
                    panel.expanded.insert(toggle_name.clone());
                }
                cx.notify();
            }))
            .child(
                svg()
                    .path(if open {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    })
                    .size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint),
            )
            .child(div().flex_1().min_w_0().truncate().child(name.clone()));
        let fields = self.fields.get(&name);
        let labelled = |label: &'static str, input: Entity<TextInput>| {
            div()
                .flex()
                .flex_col()
                .gap(tokens::SPACE_1)
                .child(div().text_color(tokens.text_quiet).child(label))
                .child(input)
        };
        let switch = |id: &'static str, key: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            let name = name.clone();
            ui::check(tokens, SharedString::from(format!("{id}-{name}")), label, on).on_click(cx.listener(
                move |_, _, _, cx| {
                    cx.emit(ExportEvent::Set {
                        name: name.clone(),
                        key,
                        value: (!on).then(|| "true".to_owned()),
                    });
                },
            ))
        };
        let editor = open.then(|| {
            div()
                .flex()
                .flex_col()
                .gap(tokens::SPACE_2)
                .when_some(fields, |column, fields| {
                    column
                        .child(labelled(crate::t!(Key::FieldTheme), fields.theme.clone()))
                        .child(labelled(crate::t!(Key::FieldOutput), fields.output.clone()))
                        .child(labelled(crate::t!(Key::FieldBleed), fields.bleed.clone()))
                })
                .child(switch(
                    "crop",
                    "crop-marks",
                    crate::t!(Key::FieldCropMarks),
                    def.crop_marks,
                    cx,
                ))
                .child(switch("draft", "draft", crate::t!(Key::FieldDraft), def.draft, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(switch("final", "final", crate::t!(Key::FieldFinal), def.is_final, cx))
                        .child(
                            div()
                                .pl(px(22.))
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(crate::t!(Key::FinalHint)),
                        ),
                )
        });
        div()
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .border_b_1()
            .border_color(tokens.divider)
            .child(header)
            .child(
                div()
                    .truncate()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(summary),
            )
            .when_some(editor, |card, editor| card.child(editor))
            .child(self.runner(name.clone(), Target::Named(name), crate::t!(Key::ExportRun), tokens, cx))
            .into_any_element()
    }
}

impl Render for ExportPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let unreviewed: usize = crate::agent::unreviewed_changes(cx).iter().map(|file| file.count).sum();
        let tokens = Tokens::get(cx).clone();
        let body = if self.setup.single {
            div()
                .p(tokens::SPACE_3)
                .flex()
                .flex_col()
                .gap(tokens::SPACE_2)
                .child(self.runner("pdf".into(), Target::Pdf, crate::t!(Key::ExportPdf), &tokens, cx))
                .child(self.runner("epub".into(), Target::Epub, crate::t!(Key::ExportEpub), &tokens, cx))
                .into_any_element()
        } else {
            let cards: Vec<_> = self
                .setup
                .defs
                .clone()
                .iter()
                .map(|def| self.card(def, &tokens, cx))
                .collect();
            let add = if self.adding {
                div()
                    .flex()
                    .flex_col()
                    .gap(tokens::SPACE_2)
                    .child(self.new_name.clone())
                    .when_some(self.add_error, |column, error| {
                        column.child(
                            div()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.accent)
                                .child(error),
                        )
                    })
                    .child(
                        ui::button(&tokens, "export-create", crate::t!(Key::ExportCreate))
                            .on_click(cx.listener(|panel, _, _, cx| panel.add(cx))),
                    )
                    .into_any_element()
            } else {
                div()
                    .id("export-add")
                    .button(&tokens, crate::t!(Key::ExportAdd))
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .text_color(tokens.text_quiet)
                    .cursor_pointer()
                    .hover(|row| row.text_color(tokens.text))
                    .on_click(cx.listener(|panel, _, window, cx| {
                        panel.adding = true;
                        panel.add_error = None;
                        let focus = panel.new_name.read(cx).focus_handle(cx);
                        focus.focus(window, cx);
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path("icons/plus.svg")
                            .size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint),
                    )
                    .child(crate::t!(Key::ExportAdd))
                    .into_any_element()
            };
            div()
                .flex()
                .flex_col()
                .when_some(self.setup.problem.clone(), |column, problem| {
                    column.child(div().p(tokens::SPACE_3).text_color(tokens.accent).child(problem))
                })
                .when(unreviewed > 0, |column| {
                    column.child(
                        div()
                            .p(tokens::SPACE_3)
                            .text_color(tokens.accent)
                            .child(crate::t!(Key::ExportUnreviewed, count = unreviewed)),
                    )
                })
                .when(cards.is_empty() && self.setup.problem.is_none(), |column| {
                    column.child(
                        div()
                            .p(tokens::SPACE_3)
                            .text_color(tokens.text_quiet)
                            .child(crate::t!(Key::NoExports)),
                    )
                })
                .children(cards)
                .child(div().p(tokens::SPACE_3).child(add))
                .into_any_element()
        };
        let status = self.status(&tokens, cx);
        div()
            .id("export")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(ui::section_label(
                &tokens,
                if self.setup.single {
                    crate::t!(Key::ExportThisDocument)
                } else {
                    crate::t!(Key::NamedExports)
                },
            ))
            .when_some(status, |panel, status| {
                panel.child(
                    div()
                        .p(tokens::SPACE_3)
                        .border_b_1()
                        .border_color(tokens.divider)
                        .child(status),
                )
            })
            .child(body)
    }
}
