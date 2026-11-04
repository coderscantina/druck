//! The Page panel: page size, margins, the text frame, and a live diagram of an even and an odd page. Every edit is a
//! change at `page.*` through the shared [`ThemeModel`], which checks it with the engine.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, Entity, Focusable, SharedString, Stateful, Subscription, Window, div};
use serde_json::{Value, json};

use super::input::{Committed, TextInput};
use super::model::ThemeModel;
use super::strings::Key;
use super::widgets;
use crate::a11y::Control as _;
use crate::theme_file::editor::Change;
use crate::theme_file::geometry::{self, Geometry, Rect, Sheet};
use crate::theme_file::tokens::{self, Group};
use crate::tokens::{self as chrome, Tokens};

/// A length under `page`, edited as text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Field {
    path: &'static [&'static str],
    label: Key,
}

impl Field {
    fn path(self) -> Vec<String> {
        std::iter::once("page")
            .chain(self.path.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    fn dotted(self) -> String {
        self.path().join(".")
    }
}

const WIDTH: Field = Field {
    path: &["size", "width"],
    label: Key::Width,
};
const HEIGHT: Field = Field {
    path: &["size", "height"],
    label: Key::Height,
};
const MARGINS: [Field; 4] = [
    Field {
        path: &["margins", "top"],
        label: Key::MarginTop,
    },
    Field {
        path: &["margins", "bottom"],
        label: Key::MarginBottom,
    },
    Field {
        path: &["margins", "inner"],
        label: Key::MarginInner,
    },
    Field {
        path: &["margins", "outer"],
        label: Key::MarginOuter,
    },
];
const COLUMN_GAP: Field = Field {
    path: &["column-gap"],
    label: Key::ColumnGap,
};
const TEXT_WIDTH: Field = Field {
    path: &["text-width"],
    label: Key::TextWidth,
};
const FIELDS: [Field; 8] = [
    WIDTH, HEIGHT, MARGINS[0], MARGINS[1], MARGINS[2], MARGINS[3], COLUMN_GAP, TEXT_WIDTH,
];

const PAPERS: [(&str, Key); 5] = [
    ("a4", Key::PaperA4),
    ("a5", Key::PaperA5),
    ("b5", Key::PaperB5),
    ("letter", Key::PaperLetter),
    ("legal", Key::PaperLegal),
];

pub struct GeometryPanel {
    model: Entity<ThemeModel>,
    inputs: Vec<(Field, Entity<TextInput>)>,
    /// The engine's refusal of the last edit, by dotted path.
    errors: Vec<(String, String)>,
    /// The field whose list of spacing tokens is open.
    open: Option<Field>,
    _subscriptions: Vec<Subscription>,
}

impl GeometryPanel {
    #[allow(dead_code, reason = "mounted by the integrator")]
    pub fn new(model: Entity<ThemeModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe_in(&model, window, |panel, _, window, cx| panel.sync(window, cx))];
        let mut inputs = Vec::new();
        for field in FIELDS {
            let input = cx.new(|cx| {
                let input = TextInput::new(window, cx);
                if field == TEXT_WIDTH {
                    input.with_placeholder(crate::t!(Key::TextWidthHint))
                } else {
                    input
                }
            });
            subscriptions.push(cx.subscribe(&input, move |panel, _, Committed(text), cx| {
                panel.commit(field, text, cx);
            }));
            inputs.push((field, input));
        }
        let mut panel = Self {
            model,
            inputs,
            errors: Vec::new(),
            open: None,
            _subscriptions: subscriptions,
        };
        panel.sync(window, cx);
        panel
    }

    /// Shows what the theme says in every field the author is not typing in.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(file) = self.model.read(cx).file() else {
            cx.notify();
            return;
        };
        let shown: Vec<_> = self
            .inputs
            .iter()
            .map(|(field, input)| {
                let path = field.path();
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                let text = file
                    .effective(&path)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                (input.clone(), text, file.get(&path).is_some())
            })
            .collect();
        for (input, text, customized) in shown {
            if !input.read(cx).focus_handle(cx).is_focused(window) {
                input.update(cx, |input, cx| input.set_value(text, customized, cx));
            }
        }
        cx.notify();
    }

    /// Empty text returns the length to the inherited value.
    fn commit(&mut self, field: Field, text: &str, cx: &mut Context<Self>) {
        let text = text.trim();
        let change = if text.is_empty() {
            Change::Remove(field.path())
        } else {
            Change::Set(field.path(), Value::String(text.to_owned()))
        };
        self.apply(field.dotted(), change, cx);
    }

    /// Makes a change. A refused one leaves the theme as it was and shows the engine's reason under its field.
    fn apply(&mut self, key: String, change: Change, cx: &mut Context<Self>) {
        let result = self.model.update(cx, |model, cx| model.change(change, cx));
        self.errors.retain(|(at, _)| *at != key);
        if let Err(message) = result {
            self.errors.push((key, message));
        }
        cx.notify();
    }

    fn error(&self, key: &str, shown: &Shown) -> Option<String> {
        self.errors
            .iter()
            .find(|(at, _)| at == key)
            .map(|(_, message)| message.clone())
            .or_else(|| shown.problem.clone())
    }

    // Rendering

    fn reset(&self, key: &str, path: Vec<String>, tokens: &Tokens, cx: &mut Context<Self>) -> Stateful<Div> {
        let owned = key.to_owned();
        widgets::link(tokens, SharedString::from(format!("reset-{key}")), Key::Reset)
            .on_click(cx.listener(move |panel, _, _, cx| panel.apply(owned.clone(), Change::Remove(path.clone()), cx)))
    }

    fn size(&self, shown: &Shown, geometry: &Geometry, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let current = shown.current.as_ref();
        let custom = current.is_some_and(Value::is_object);
        let key = "page.size";
        let mut items: Vec<_> = PAPERS
            .iter()
            .map(|&(name, label)| {
                let item = div()
                    .id(SharedString::from(format!("paper-{name}")))
                    .button(tokens, label)
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.apply(key.to_owned(), Change::set(&["page", "size"], name.into()), cx)
                    }));
                (item, current.and_then(Value::as_str) == Some(name))
            })
            .collect();
        let (width, height) = geometry
            .width
            .zip(geometry.height)
            .or_else(|| geometry::paper("a4"))
            .unwrap_or_default();
        let written = |points: f64| format!("{}mm", geometry::rounded(geometry::to_mm(points)));
        let size = json!({ "width": written(width), "height": written(height) });
        items.push((
            div()
                .id("paper-custom")
                .toggle(tokens, crate::t!(Key::CustomSize), custom)
                .child(crate::t!(Key::CustomSize))
                .on_click(cx.listener(move |panel, _, _, cx| {
                    if !custom {
                        panel.apply(key.to_owned(), Change::set(&["page", "size"], size.clone()), cx);
                    }
                })),
            custom,
        ));
        let path = vec!["page".to_owned(), "size".to_owned()];
        let end = div()
            .flex()
            .when(shown.own, |end| end.child(self.reset(key, path, tokens, cx)));
        let control = div()
            .flex()
            .flex_col()
            .child(widgets::segmented(tokens, items))
            .when_some(self.error(key, shown), |control, message| {
                control.child(widgets::error_line(tokens, message))
            });
        widgets::field(tokens, Key::PageSize, end, control)
    }

    fn mirror(&self, shown: &Shown, on: bool, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let key = "page.margins.mirror";
        let items = [(true, Key::Yes), (false, Key::No)]
            .into_iter()
            .map(|(value, label)| {
                let item = div()
                    .id(SharedString::from(format!("mirror-{value}")))
                    .button(tokens, crate::t!(label))
                    .child(label)
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        let change = Change::set(&["page", "margins", "mirror"], Value::Bool(value));
                        panel.apply(key.to_owned(), change, cx);
                    }));
                (item, on == value)
            })
            .collect();
        let path = ["page", "margins", "mirror"].map(str::to_owned).to_vec();
        let end = div()
            .flex()
            .when(shown.own, |end| end.child(self.reset(key, path, tokens, cx)));
        let control = div()
            .flex()
            .flex_col()
            .child(widgets::segmented(tokens, items))
            .when_some(self.error(key, shown), |control, message| {
                control.child(widgets::error_line(tokens, message))
            });
        widgets::field(tokens, Key::Mirror, end, control)
    }

    /// One length: its label with what it comes to, the token list link, and Reset; then the field and the
    /// engine's refusal.
    fn length(&self, field: Field, snapshot: &Snapshot, tokens: &Tokens, cx: &mut Context<Self>) -> Div {
        let Some(shown) = snapshot.field(field) else {
            return div();
        };
        let dotted = field.dotted();
        let open = self.open == Some(field);
        let id = |what: &str| SharedString::from(format!("{what}-{dotted}"));
        let end = div()
            .flex()
            .items_center()
            .gap(chrome::SPACE_2)
            .when_some(shown.note.clone(), |end, note| end.child(widgets::note(tokens, note)))
            .child(
                widgets::link(tokens, id("pick"), if open { "▴ $" } else { "▾ $" }).on_click(cx.listener(
                    move |panel, _, _, cx| {
                        panel.open = if panel.open == Some(field) { None } else { Some(field) };
                        cx.notify();
                    },
                )),
            )
            .when(shown.own, |end| {
                end.child(self.reset(&dotted, field.path(), tokens, cx))
            });
        let input = self
            .inputs
            .iter()
            .find(|(at, _)| *at == field)
            .map(|(_, input)| input.clone());
        let row = div()
            .flex()
            .items_center()
            .gap(chrome::SPACE_2)
            .when_some(input, |row, input| row.child(div().flex_1().min_w_0().child(input)))
            .when(field == TEXT_WIDTH, |row| {
                row.child(
                    widgets::button(tokens, id("full"), Key::FullFrame).on_click(cx.listener(|panel, _, _, cx| {
                        panel.apply(TEXT_WIDTH.dotted(), Change::Set(TEXT_WIDTH.path(), Value::Null), cx)
                    })),
                )
            });
        let control = div()
            .flex()
            .flex_col()
            .child(row)
            .when(open, |control| {
                control.child(self.token_list(field, &snapshot.spacing, shown, tokens, cx))
            })
            .when_some(self.error(&dotted, shown), |control, message| {
                control.child(widgets::error_line(tokens, message))
            });
        widgets::field(tokens, field.label, end, control)
    }

    fn token_list(
        &self,
        field: Field,
        listed: &[(String, String, Option<String>)],
        shown: &Shown,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let current = shown.current.as_ref().and_then(Value::as_str);
        let items = listed
            .iter()
            .map(|(written, name, detail)| {
                let value = written.clone();
                widgets::menu_item(
                    tokens,
                    SharedString::from(format!("token-{written}")),
                    name.clone(),
                    detail.clone(),
                    current == Some(written.as_str()),
                )
                .on_click(cx.listener(move |panel, _, _, cx| {
                    panel.open = None;
                    panel.apply(
                        field.dotted(),
                        Change::Set(field.path(), Value::String(value.clone())),
                        cx,
                    );
                }))
            })
            .collect();
        widgets::menu(tokens, items)
    }
}

/// An even and an odd page side by side, as a spread, with the frame's size under them.
fn diagram(geometry: &Geometry, tokens: &Tokens) -> Div {
    let (Some(diagram), Some(frame)) = (geometry.diagram(), geometry.frame()) else {
        return div();
    };
    let size = format!(
        "{} × {} mm",
        number(geometry::to_mm(frame.width)),
        number(geometry::to_mm(frame.height))
    );
    div()
        .px(chrome::SPACE_3)
        .pb(chrome::SPACE_3)
        .flex()
        .flex_col()
        .items_center()
        .gap(chrome::SPACE_2)
        .child(
            div()
                .flex()
                .gap(chrome::SPACE_1)
                .child(sheet(&diagram.even, diagram.aspect, tokens))
                .child(sheet(&diagram.odd, diagram.aspect, tokens)),
        )
        .child(widgets::note(tokens, crate::t!(Key::FrameSize, value = &size)))
}

fn sheet(sheet: &Sheet, aspect: f64, tokens: &Tokens) -> Div {
    let height = chrome::COVER_PREVIEW_HEIGHT;
    let width = height * aspect as f32;
    let place = |rect: Rect| {
        div()
            .absolute()
            .left(width * rect.x as f32)
            .top(height * rect.y as f32)
            .w(width * rect.width as f32)
            .h(height * rect.height as f32)
    };
    div()
        .relative()
        .flex_none()
        .w(width)
        .h(height)
        .bg(tokens.sheet)
        .border_1()
        .border_color(tokens.divider)
        .child(place(sheet.prose).bg(tokens.text_faint).opacity(0.45))
        .when_some(sheet.gap, |page, gap| page.child(place(gap).bg(tokens.sheet)))
        .child(place(sheet.frame).border_1().border_color(tokens.divider))
}

fn number(value: f64) -> String {
    geometry::rounded(value).replace('.', crate::t!(Key::Decimal))
}

/// What a length comes to, in millimetres and points.
fn describe(points: f64) -> String {
    crate::t!(
        Key::LengthNote,
        mm = number(geometry::to_mm(points)),
        pt = number(points)
    )
}

/// What a property shows, read from the file once so building the panel does not hold the model.
struct Shown {
    current: Option<Value>,
    own: bool,
    note: Option<String>,
    problem: Option<String>,
}

struct Snapshot {
    geometry: Geometry,
    size: Shown,
    mirror: Shown,
    fields: Vec<(Field, Shown)>,
    /// The spacing tokens while a list is open: written form, name, and what each comes to.
    spacing: Vec<(String, String, Option<String>)>,
}

impl Snapshot {
    fn read(model: &ThemeModel, open: bool) -> Result<Self, String> {
        let file = model.file().map_err(|message| {
            if message.is_empty() {
                crate::t!(Key::NoTheme).to_owned()
            } else {
                message.to_owned()
            }
        })?;
        let theme = file.merged();
        let shown = |path: &[&str]| {
            let full: Vec<&str> = std::iter::once("page").chain(path.iter().copied()).collect();
            let current = file.effective(&full).cloned();
            Shown {
                note: current
                    .as_ref()
                    .and_then(|value| geometry::length(theme, value))
                    .map(describe),
                own: file.get(&full).is_some(),
                problem: model.problem_at(&full.join(".")).map(|problem| problem.message.clone()),
                current,
            }
        };
        let spacing = if open {
            tokens::of(theme, Group::Spacing)
                .into_iter()
                .map(|(name, value)| {
                    let detail = geometry::length(theme, &value)
                        .map(describe)
                        .or_else(|| value.as_str().map(str::to_owned));
                    (Group::Spacing.reference(&name), name, detail)
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(Self {
            geometry: Geometry::read(theme),
            size: shown(&["size"]),
            mirror: shown(&["margins", "mirror"]),
            fields: FIELDS.iter().map(|field| (*field, shown(field.path))).collect(),
            spacing,
        })
    }

    fn field(&self, field: Field) -> Option<&Shown> {
        self.fields.iter().find(|(at, _)| *at == field).map(|(_, shown)| shown)
    }
}

impl Render for GeometryPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = &Tokens::get(cx).clone();
        let column = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(widgets::header(tokens, crate::t!(Key::GeometryTitle).to_uppercase()));
        let snapshot = match Snapshot::read(self.model.read(cx), self.open.is_some()) {
            Ok(snapshot) => snapshot,
            Err(message) => {
                return column.child(
                    div()
                        .p(chrome::SPACE_3)
                        .text_color(tokens.text_quiet)
                        .child(SharedString::from(message)),
                );
            }
        };
        let custom = snapshot.size.current.as_ref().is_some_and(Value::is_object);
        let mut rows: Vec<AnyElement> = vec![
            diagram(&snapshot.geometry, tokens).into_any_element(),
            self.size(&snapshot.size, &snapshot.geometry, tokens, cx)
                .into_any_element(),
        ];
        if custom {
            for field in [WIDTH, HEIGHT] {
                rows.push(self.length(field, &snapshot, tokens, cx).into_any_element());
            }
        }
        rows.push(widgets::section(tokens, crate::t!(Key::Margins)).into_any_element());
        for field in MARGINS {
            rows.push(self.length(field, &snapshot, tokens, cx).into_any_element());
        }
        rows.push(
            self.mirror(&snapshot.mirror, snapshot.geometry.mirror, tokens, cx)
                .into_any_element(),
        );
        rows.push(widgets::section(tokens, crate::t!(Key::Columns)).into_any_element());
        for field in [COLUMN_GAP, TEXT_WIDTH] {
            rows.push(self.length(field, &snapshot, tokens, cx).into_any_element());
        }
        column.child(
            div()
                .id("geometry")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt(chrome::SPACE_2)
                .children(rows),
        )
    }
}
