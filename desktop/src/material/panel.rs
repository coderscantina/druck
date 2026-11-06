//! The material panel: one column that shows a kind's list or one item, with a back arrow and a trail of the last places
//! in its header, and a field that filters the list and, while it has the keys, jumps to any item. There are no tabs.
//!
//! Keys: up and down choose a row, or a passage of an item; Return opens, Shift+Return inserts, Command+Return pins to
//! the paragraph at the caret; `Cmd+[` and `Cmd+]` walk the trail; Escape gives the keys back to the text.

use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight, HighlightStyle,
    InteractiveText, KeyBinding, MouseButton, ObjectFit, Role, ScrollHandle, SharedString, StyledText, Subscription,
    Window, actions, div, ease_out_quint, img, svg,
};

use super::model::{Form, Item, Kind, Where, mentions};
use super::trail::{Crumb, Place, Trail};
use super::{Key, Material, MaterialChanged, icon, insert_label, kind_label};
use crate::a11y::Control as _;
use crate::editor::EditorView;
use crate::editor::review::small_button;
use crate::inspector::input::{self, Cancelled, Edited, Submitted, TextInput};
use crate::tokens::{self, Tokens};

const CONTEXT: &str = "Material";

actions!(material_panel, [Up, Down, Open, Insert, Pin, Back, Forward, Leave]);

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", Up, context),
        KeyBinding::new("down", Down, context),
        KeyBinding::new("enter", Open, context),
        KeyBinding::new("shift-enter", Insert, context),
        KeyBinding::new("cmd-enter", Pin, context),
        KeyBinding::new("cmd-[", Back, context),
        KeyBinding::new("cmd-]", Forward, context),
        KeyBinding::new("escape", Leave, context),
    ]);
}

/// An image's size in pixels, or why it cannot be read.
type ImageSize = Result<(f32, f32), String>;

/// A row of a list: a group's label before it, if it starts one, and the item with what stands at its end.
struct Row {
    group: Option<&'static str>,
    id: String,
    end: Option<String>,
}

pub struct MaterialPanel {
    material: Entity<Material>,
    editor: Entity<EditorView>,
    trail: Trail,
    field: Entity<TextInput>,
    focus: FocusHandle,
    /// The chosen row of the list or the jump list.
    selected: usize,
    /// The chosen paragraph of an open item's text, which Shift+Return quotes and Command+Return pins.
    passage: Option<usize>,
    /// An open table's rows, read when it opened.
    table: Option<(String, Vec<Vec<String>>)>,
    /// An open image's size, read when it opened.
    image_size: Option<(String, ImageSize)>,
    scroll: ScrollHandle,
    /// Counts the places arrived at, so each one's content slides in afresh, and whether the last move went down.
    arrival: (usize, bool),
    pub(super) probe: super::probe::ProbeChoice,
    _subscriptions: Vec<Subscription>,
}

impl MaterialPanel {
    pub fn new(
        material: Entity<Material>,
        editor: Entity<EditorView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextInput::new(window, cx)
                .bare()
                .keep_focus_on_enter()
                .arrows_to_parent()
                .with_label(Key::FilterOrJump)
                .with_placeholder(Key::FilterOrJump);
            field.pass_command_enter();
            field
        });
        let subscriptions = vec![
            cx.subscribe(&field, |panel, _, _: &Edited, cx| {
                panel.selected = 0;
                cx.notify();
            }),
            cx.subscribe_in(&field, window, |panel, _, _: &Submitted, window, cx| {
                panel.open(window, cx)
            }),
            cx.subscribe_in(&field, window, |panel, _, _: &Cancelled, window, cx| {
                panel.selected = 0;
                window.focus(&panel.focus, cx);
                cx.notify();
            }),
            cx.subscribe(&material, |panel, _, _: &MaterialChanged, cx| {
                let material = panel.material.read(cx);
                let items = material.items().clone();
                panel.trail.retain(|id| items.iter().any(|item| item.id == id));
                cx.notify();
            }),
        ];
        Self {
            material,
            editor,
            trail: Trail::new(Kind::Research),
            field,
            focus: cx.focus_handle(),
            selected: 0,
            arrival: (0, true),
            passage: None,
            table: None,
            image_size: None,
            scroll: ScrollHandle::new(),
            probe: super::probe::ProbeChoice::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Gives the panel the keys: its field, or its list.
    pub fn take_keys(&mut self, field: bool, window: &mut Window, cx: &mut Context<Self>) {
        if field {
            self.field.update(cx, |field, cx| field.select_all(cx));
            let handle = self.field.focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Shows a kind's list, from its rail tab.
    pub fn show_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        if *self.trail.current() == Place::List(kind) {
            return;
        }
        self.trail.list(kind);
        self.arrived(false, cx);
    }

    /// Shows an item under its kind's list, as a row, the jump field, or the library opens it.
    pub fn open_item(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(kind) = self.material.read(cx).item(id).map(|item| item.kind) else {
            return;
        };
        self.trail.open(id, kind);
        self.arrived(true, cx);
    }

    /// Follows a source the open item names, one level down the breadcrumb.
    fn follow_item(&mut self, id: &str, cx: &mut Context<Self>) {
        self.trail.follow(id);
        self.arrived(true, cx);
    }

    /// Starts the new place at its top. What it needs from its files is read as it is drawn, since this may run while
    /// the material is busy asking for it.
    fn arrived(&mut self, down: bool, cx: &mut Context<Self>) {
        self.arrival = (self.arrival.0 + 1, down);
        self.selected = 0;
        self.passage = None;
        self.scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }

    /// Reads an open table's rows or an image's size, once for each item.
    fn load(&mut self, cx: &mut Context<Self>) {
        if let Place::Item(id) = self.trail.current().clone() {
            let material = self.material.read(cx);
            let file = material
                .item(&id)
                .and_then(|item| Some((item.form, material.base()?.join(item.path()?))));
            match file {
                Some((Form::Table, file)) if self.table.as_ref().is_none_or(|(seen, _)| *seen != id) => {
                    let rows = std::fs::read_to_string(file).map_or(Vec::new(), |text| super::model::csv_rows(&text));
                    self.table = Some((id.clone(), rows));
                }
                Some((Form::Image, file)) if self.image_size.as_ref().is_none_or(|(seen, _)| *seen != id) => {
                    self.image_size = Some((id.clone(), crate::editor::rich::images::size_of(&file)));
                }
                _ => {}
            }
            let editor = self.editor.clone();
            self.probe.item_opened(&id, &editor, cx);
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        if self.trail.up() {
            self.arrived(false, cx);
        }
    }

    fn forward(&mut self, cx: &mut Context<Self>) {
        if self.trail.down() {
            self.arrived(true, cx);
        }
    }

    fn query(&self, cx: &App) -> String {
        self.field.read(cx).text().trim().to_owned()
    }

    fn jumping(&self, window: &Window, cx: &App) -> bool {
        self.field.focus_handle(cx).is_focused(window)
    }

    /// Where an item is used in the open chapter, for the end of its row.
    fn where_of(&self, id: &str, cx: &App) -> Option<String> {
        let uses = self.material.read(cx).uses_of(id)?;
        Some(match &uses.shown {
            Where::Figure(section) | Where::Section(section) => section.clone(),
            Where::Cited(count) => crate::t!(Key::CitedCount, count = count),
        })
    }

    /// The rows the panel lists now: the jump list while the field has the keys, else a kind's list, filtered.
    fn rows(&self, window: &Window, cx: &App) -> Vec<Row> {
        let material = self.material.read(cx);
        let items = material.items();
        let query = self.query(cx);
        let matches =
            |item: &Item| query.is_empty() || !super::model::matching(std::slice::from_ref(item), &query).is_empty();
        let mut rows = Vec::new();
        if self.jumping(window, cx) {
            let recent: Vec<&Item> = (self.trail.recent().iter())
                .filter_map(|id| material.item(id))
                .filter(|item| matches(item))
                .collect();
            for (at, item) in recent.iter().enumerate() {
                rows.push(Row {
                    group: (at == 0).then(|| crate::t!(Key::Recent)),
                    id: item.id.clone(),
                    end: Some(kind_label(item.kind).to_owned()),
                });
            }
            let others = items
                .iter()
                .filter(|item| matches(item) && !recent.iter().any(|seen| seen.id == item.id));
            for (at, item) in others.enumerate() {
                rows.push(Row {
                    group: (at == 0).then(|| crate::t!(Key::Matches)),
                    id: item.id.clone(),
                    end: self
                        .where_of(&item.id, cx)
                        .or_else(|| Some(kind_label(item.kind).to_owned())),
                });
            }
            return rows;
        }
        let Place::List(kind) = self.trail.current() else {
            return rows;
        };
        let used: Vec<&Item> = (material.uses().iter())
            .filter_map(|uses| material.item(&uses.item))
            .filter(|item| item.kind == *kind && matches(item))
            .collect();
        for (at, item) in used.iter().enumerate() {
            rows.push(Row {
                group: (at == 0).then(|| crate::t!(Key::UsedHere)),
                id: item.id.clone(),
                end: self.where_of(&item.id, cx),
            });
        }
        let rest = (items.iter())
            .filter(|item| item.kind == *kind && matches(item) && !used.iter().any(|seen| seen.id == item.id));
        for (at, item) in rest.enumerate() {
            rows.push(Row {
                // The header already names the kind, so its label only separates it from the used ones.
                group: (at == 0 && !used.is_empty()).then(|| kind_label(*kind)),
                id: item.id.clone(),
                end: None,
            });
        }
        rows
    }

    fn current_item(&self, cx: &App) -> Option<Item> {
        let Place::Item(id) = self.trail.current() else {
            return None;
        };
        self.material.read(cx).item(id).cloned()
    }

    fn step(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.jumping(window, cx)
            && let Some(item) = self.current_item(cx)
        {
            let count = item.paragraphs().len();
            if count > 0 {
                self.passage = Some(match (self.passage, down) {
                    (None, true) => 0,
                    (None, false) => count - 1,
                    (Some(at), true) => (at + 1).min(count - 1),
                    (Some(at), false) => at.saturating_sub(1),
                });
                cx.notify();
            }
            return;
        }
        let count = self.rows(window, cx).len();
        if count > 0 {
            self.selected = if down {
                (self.selected + 1).min(count - 1)
            } else {
                self.selected.saturating_sub(1)
            };
            self.scroll.scroll_to_item(self.selected);
            cx.notify();
        }
    }

    /// The row chosen in a list, or the open item with its chosen passage.
    fn chosen(&self, window: &Window, cx: &App) -> Option<(String, Option<String>)> {
        if !self.jumping(window, cx)
            && let Some(item) = self.current_item(cx)
        {
            let passage = self.passage.and_then(|at| item.paragraphs().into_iter().nth(at));
            return Some((item.id, passage));
        }
        let rows = self.rows(window, cx);
        rows.get(self.selected).map(|row| (row.id.clone(), None))
    }

    /// Return: a list's row opens; in an item, the chosen passage's first mention is followed.
    fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.jumping(window, cx) {
            if let Some((id, _)) = self.chosen(window, cx) {
                self.field
                    .update(cx, |field, cx| field.set_value(String::new(), true, cx));
                window.focus(&self.focus, cx);
                self.open_item(&id, cx);
            }
            return;
        }
        match self.current_item(cx) {
            Some(item) => {
                if let Some(id) = self.followed(&item, cx) {
                    self.follow_item(&id, cx);
                }
            }
            None => {
                if let Some((id, _)) = self.chosen(window, cx) {
                    self.open_item(&id, cx);
                }
            }
        }
    }

    /// The first source the open item's chosen passage names, which Return follows.
    fn followed(&self, item: &Item, cx: &App) -> Option<String> {
        let paragraph = item.paragraphs().into_iter().nth(self.passage?)?;
        let items = self.material.read(cx).items();
        mentions(&paragraph, items, &item.id)
            .into_iter()
            .next()
            .map(|(_, id)| id)
    }

    fn insert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, passage)) = self.chosen(window, cx) {
            self.material
                .update(cx, |material, cx| material.insert(&id, passage.as_deref(), window, cx));
        }
    }

    fn pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, passage)) = self.chosen(window, cx) {
            self.material
                .update(cx, |material, cx| material.pin(&id, passage.as_deref(), cx));
        }
    }

    fn leave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.focus_handle(cx);
        window.focus(&editor, cx);
    }

    /// A place's name on the trail.
    fn place_name(&self, place: &Place, cx: &App) -> String {
        match place {
            Place::List(kind) => kind_label(*kind).to_owned(),
            Place::Item(id) => {
                let name = id.rsplit('/').next().unwrap_or(id);
                let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
                let _ = cx;
                stem.trim_start_matches('@').to_owned()
            }
        }
    }

    fn header(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let can_go_back = self.trail.can_go_up();
        let back = div()
            .id("material-back")
            .button(tokens, crate::t!(Key::Back))
            .flex_none()
            .size(tokens::SMALL_BUTTON_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .rounded(tokens::RADIUS)
            .when(can_go_back, |button| {
                button.cursor_pointer().hover(|button| button.bg(tokens.raised))
            })
            .child(
                svg()
                    .path("icons/chevron-left.svg")
                    .size(tokens::ICON_SIZE)
                    .text_color(if can_go_back {
                        tokens.text_quiet
                    } else {
                        tokens.text_faint
                    }),
            )
            .on_click(cx.listener(|panel, _, _, cx| panel.back(cx)));
        let crumbs = self.trail.crumbs();
        let last = crumbs.len().saturating_sub(1);
        let mut trail: Vec<AnyElement> = Vec::new();
        for (at, crumb) in crumbs.into_iter().enumerate() {
            if at > 0 {
                trail.push(
                    svg()
                        .flex_none()
                        .path("icons/chevron-right.svg")
                        .size(tokens::MATERIAL_TRAIL_ARROW)
                        .text_color(tokens.text_faint)
                        .into_any_element(),
                );
            }
            let Crumb::Place(depth, place) = crumb else {
                trail.push(
                    div()
                        .flex_none()
                        .text_color(tokens.text_faint)
                        .child("\u{2026}")
                        .into_any_element(),
                );
                continue;
            };
            let name = self.place_name(place, cx);
            let step = div()
                .id(("crumb", depth))
                .px(tokens::SPACE_1)
                .rounded(tokens::BOX_RADIUS)
                .truncate();
            if at == last {
                // The place just reached comes in from the side it was reached from.
                let (arrival, down) = self.arrival;
                let from = if down {
                    tokens::OUTLINE_SLIDE
                } else {
                    gpui::px(0.) - tokens::OUTLINE_SLIDE
                };
                trail.push(
                    step.flex_shrink(1.)
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.text)
                        .child(name)
                        .with_animation(
                            ("crumb-in", arrival),
                            Animation::new(tokens::FADE).with_easing(ease_out_quint()),
                            move |step, shown| step.opacity(shown).ml(from * (1. - shown)),
                        )
                        .into_any_element(),
                );
                continue;
            }
            trail.push(
                step.flex_shrink(1.)
                    .min_w_0()
                    .max_w(tokens::MATERIAL_TRAIL_STEP)
                    .text_color(tokens.text_quiet)
                    .cursor_pointer()
                    .hover(|step| step.bg(tokens.raised).text_color(tokens.text))
                    .on_click(cx.listener(move |panel, _, _, cx| {
                        panel.trail.up_to(depth);
                        panel.arrived(false, cx);
                    }))
                    .child(name)
                    .into_any_element(),
            );
        }
        div()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .border_b_1()
            .border_color(tokens.divider)
            .child(back)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(tokens::HAIR_GAP)
                    .text_size(tokens::TEXT_SMALL + gpui::px(1.))
                    .children(trail),
            )
    }

    fn field_row(&self, jumping: bool, tokens: &Tokens) -> impl IntoElement + use<> {
        let hint = if jumping { "esc" } else { "\u{2318}L" };
        div()
            .flex_none()
            .mx(tokens::SPACE_3)
            .my(tokens::SPACE_2)
            .h(tokens::MATERIAL_ROW_HEIGHT)
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .bg(tokens.field)
            .border_1()
            .border_color(if jumping { tokens.divider } else { tokens.field })
            .child(
                svg()
                    .flex_none()
                    .path("icons/search.svg")
                    .size(tokens::NOTE_ICON)
                    .text_color(tokens.text_faint),
            )
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .child(
                div()
                    .flex_none()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(hint),
            )
    }

    fn list(&self, window: &Window, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.rows(window, cx);
        if rows.is_empty() {
            let text = match self.trail.current() {
                Place::List(kind) if self.query(cx).is_empty() => crate::t!(
                    Key::NothingHere,
                    folder = match kind {
                        Kind::Notes => "notes/",
                        Kind::Probes => "notes/probes/",
                        _ => "research/",
                    }
                ),
                _ => crate::t!(Key::NoMatch).to_owned(),
            };
            return div()
                .px(tokens::SPACE_3)
                .py(tokens::SPACE_2)
                .text_color(tokens.text_faint)
                .child(text)
                .into_any_element();
        }
        let material = self.material.read(cx);
        let mut children: Vec<AnyElement> = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            if let Some(group) = row.group {
                children.push(
                    div()
                        .px(tokens::SPACE_3)
                        .pt(tokens::SPACE_2)
                        .pb(tokens::HAIR_GAP)
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(group)
                        .into_any_element(),
                );
            }
            let Some(item) = material.item(&row.id) else {
                continue;
            };
            let chosen = at == self.selected;
            let id = row.id.clone();
            children.push(
                div()
                    .id(SharedString::from(format!("material-row-{}", row.id)))
                    .role(Role::ListItem)
                    .aria_label(item.label().to_owned())
                    .aria_selected(chosen)
                    .h(tokens::MATERIAL_ROW_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .cursor_pointer()
                    .when(chosen, |line| line.bg(tokens.raised))
                    .hover(|line| line.bg(tokens.raised))
                    .child(
                        svg()
                            .flex_none()
                            .path(icon(item))
                            .size(tokens::ICON_SIZE)
                            .text_color(tokens.text_faint),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.text)
                            .child(item.label().to_owned()),
                    )
                    .when_some(row.end.clone(), |line, end| {
                        line.child(
                            div()
                                .flex_none()
                                .max_w(tokens::MATERIAL_TRAIL_STEP * 1.5)
                                .truncate()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(end),
                        )
                    })
                    .on_click(cx.listener(move |panel, _, window, cx| {
                        panel
                            .field
                            .update(cx, |field, cx| field.set_value(String::new(), true, cx));
                        window.focus(&panel.focus, cx);
                        panel.open_item(&id, cx);
                    }))
                    .into_any_element(),
            );
        }
        div().flex().flex_col().children(children).into_any_element()
    }

    fn item_view(&self, item: &Item, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let (items, uses, base) = {
            let material = self.material.read(cx);
            (
                material.items().clone(),
                material.uses_of(&item.id).cloned(),
                material.base().cloned(),
            )
        };
        let path = item.path().unwrap_or(item.id.as_str()).to_owned();
        let meta = match item.form {
            Form::Image => match self.image_size.as_ref().filter(|(seen, _)| *seen == item.id) {
                Some((_, Ok((width, height)))) => format!("{path} \u{b7} {width} \u{d7} {height}"),
                _ => path,
            },
            Form::Table => match self.table.as_ref().filter(|(seen, _)| *seen == item.id) {
                Some((_, rows)) => format!(
                    "{path} \u{b7} {}",
                    crate::t!(Key::Rows, count = rows.len().saturating_sub(1))
                ),
                None => path,
            },
            Form::Source => format!("{} \u{b7} {}", item.id, crate::t!(Key::Entry)),
            Form::Text if item.kind == Kind::Probes => format!(
                "{} \u{b7} {path} \u{b7} {}",
                crate::t!(Key::Probe),
                crate::t!(Key::Words, count = item.words())
            ),
            Form::Text => match &item.date {
                Some(date) => format!("{path} \u{b7} {date}"),
                None => path,
            },
        };
        let cited = uses
            .as_ref()
            .filter(|uses| !matches!(uses.shown, Where::Figure(_)))
            .map(|uses| {
                let count = match uses.shown {
                    Where::Cited(count) => count,
                    _ => 1,
                };
                let lead = match count {
                    1 => crate::t!(Key::CitedOnce).to_owned(),
                    2 => crate::t!(Key::CitedTwice).to_owned(),
                    _ => crate::t!(Key::CitedTimes, count = count),
                };
                let mut links: Vec<AnyElement> = Vec::new();
                for (at, (section, offset)) in uses.sections.iter().enumerate() {
                    let offset = *offset;
                    let comma = at + 1 < uses.sections.len();
                    links.push(
                        div()
                            .id(("cited-in", at))
                            .role(Role::Link)
                            .aria_label(section.clone())
                            .cursor_pointer()
                            .text_color(tokens.link)
                            .hover(|link| link.underline())
                            .child(if comma { format!("{section},") } else { section.clone() })
                            .on_click(cx.listener(move |panel, _, window, cx| {
                                panel
                                    .material
                                    .update(cx, |material, cx| material.show_in_text(offset, window, cx));
                            }))
                            .into_any_element(),
                    );
                }
                div()
                    .mt(tokens::SPACE_2)
                    .flex()
                    .flex_wrap()
                    .gap(tokens::SPACE_1)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(lead)
                    .children(links)
            });
        let id = item.id.clone();
        let insert = small_button("material-insert", insert_label(item).to_owned(), tokens)
            .text_color(tokens.accent)
            .on_click(cx.listener({
                let id = id.clone();
                move |panel, _, window, cx| {
                    let passage = panel.passage_text(cx);
                    panel
                        .material
                        .update(cx, |material, cx| material.insert(&id, passage.as_deref(), window, cx));
                }
            }));
        let pin = small_button("material-pin", crate::t!(Key::PinHere).to_owned(), tokens).on_click(cx.listener(
            move |panel, _, _, cx| {
                let passage = panel.passage_text(cx);
                panel
                    .material
                    .update(cx, |material, cx| material.pin(&id, passage.as_deref(), cx));
            },
        ));
        let probe_choice = (item.kind == Kind::Probes).then(|| self.probe.choice(tokens, cx));
        let header = div()
            .px(tokens::SPACE_4)
            .pt(tokens::SPACE_3)
            .pb(tokens::SPACE_3)
            .border_b_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(tokens::SPACE_2)
                    .child(
                        div()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(item.title.clone()),
                    )
                    .children(probe_choice),
            )
            .child(
                div()
                    .mt(tokens::HAIR_GAP)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(meta),
            )
            .children(cited)
            .child(
                div()
                    .mt(tokens::SPACE_3)
                    .flex()
                    .flex_wrap()
                    .gap(tokens::SPACE_1)
                    .child(insert)
                    .child(pin),
            );
        let body = match item.form {
            Form::Text => self.reading(item, &items, tokens, cx),
            Form::Image => match base.as_ref().zip(item.path()) {
                Some((base, path)) => div()
                    .px(tokens::SPACE_4)
                    .py(tokens::SPACE_3)
                    .child(
                        img(base.join(path))
                            .w_full()
                            .h(tokens::MATERIAL_IMAGE_HEIGHT)
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element(),
                None => div().into_any_element(),
            },
            Form::Table => self.table_view(item, tokens),
            Form::Source => div().into_any_element(),
        };
        div().flex().flex_col().child(header).child(body).into_any_element()
    }

    fn passage_text(&self, cx: &App) -> Option<String> {
        let item = self.current_item(cx)?;
        item.paragraphs().into_iter().nth(self.passage?)
    }

    /// A text's paragraphs in the book's face at a small size, the chosen one in full ink, names of other items as links.
    fn reading(&self, item: &Item, items: &Rc<Vec<Item>>, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let (family, size, line) = match self.probe.type_for(item) {
            Some(face) => face,
            None => (
                self.editor.read(cx).body_font().family,
                tokens::MATERIAL_READING_TEXT,
                tokens::MATERIAL_READING_LINE,
            ),
        };
        let paragraphs = item.paragraphs();
        if paragraphs.is_empty() {
            return div().into_any_element();
        }
        let link = HighlightStyle {
            color: Some(tokens.link.into()),
            ..Default::default()
        };
        let mut children: Vec<AnyElement> = Vec::new();
        for (at, paragraph) in paragraphs.into_iter().enumerate() {
            let chosen = self.passage == Some(at);
            let found = mentions(&paragraph, items, &item.id);
            let ranges: Vec<_> = found.iter().map(|(range, _)| range.clone()).collect();
            let targets: Vec<String> = found.into_iter().map(|(_, id)| id).collect();
            let styled = StyledText::new(SharedString::from(paragraph))
                .with_highlights(ranges.iter().map(|range| (range.clone(), link)));
            let panel = cx.entity().downgrade();
            let text = InteractiveText::new(("passage", at), styled).on_click(ranges, move |index, _, cx| {
                if let (Some(panel), Some(id)) = (panel.upgrade(), targets.get(index)) {
                    panel.update(cx, |panel, cx| panel.follow_item(id, cx));
                }
            });
            children.push(
                div()
                    .id(("paragraph", at))
                    .mb(tokens::SPACE_2)
                    .px(tokens::SPACE_1)
                    .rounded(tokens::BOX_RADIUS)
                    .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                    .when(chosen, |paragraph| paragraph.bg(tokens.raised))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |panel, _, _, cx| {
                            panel.passage = Some(at);
                            cx.notify();
                        }),
                    )
                    .child(text)
                    .into_any_element(),
            );
        }
        div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_3)
            .font_family(family)
            .text_size(size)
            .line_height(line)
            .children(children)
            .when_some(self.probe.note(item), |body, note| {
                body.child(
                    div()
                        .mt(tokens::SPACE_2)
                        .px(tokens::SPACE_1)
                        .font_family(tokens::FONT_UI)
                        .text_size(tokens::TEXT_SMALL)
                        .line_height(tokens::PIN_LINE_HEIGHT)
                        .text_color(tokens.text_faint)
                        .child(note),
                )
            })
            .into_any_element()
    }

    fn table_view(&self, item: &Item, tokens: &Tokens) -> AnyElement {
        let Some((_, rows)) = self.table.as_ref().filter(|(seen, _)| *seen == item.id) else {
            return div().into_any_element();
        };
        if rows.is_empty() {
            return div()
                .p(tokens::SPACE_4)
                .text_color(tokens.text_faint)
                .child(crate::t!(Key::CannotRead))
                .into_any_element();
        }
        let lines: Vec<AnyElement> = (rows.iter().take(tokens::MATERIAL_TABLE_ROWS).enumerate())
            .map(|(at, row)| {
                div()
                    .flex()
                    .gap(tokens::SPACE_2)
                    .py(tokens::HAIR_GAP)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .when(at == 0, |line| {
                        line.font_weight(FontWeight::SEMIBOLD).text_color(tokens.text)
                    })
                    .children(
                        row.iter()
                            .map(|cell| div().flex_1().min_w_0().truncate().child(cell.clone())),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .px(tokens::SPACE_4)
            .py(tokens::SPACE_3)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .children(lines)
            .into_any_element()
    }

    /// The keys of the verbs that act now: Insert and Pin once something is chosen, Open once there is a row to open or
    /// a passage that names a source.
    fn foot(&self, window: &Window, tokens: &Tokens, cx: &App) -> impl IntoElement + use<> {
        let chosen = self.chosen(window, cx).is_some();
        let opens = match (self.jumping(window, cx), self.current_item(cx)) {
            (false, Some(item)) => self.followed(&item, cx).is_some(),
            _ => chosen,
        };
        div()
            .flex_none()
            .h(tokens::MATERIAL_ROW_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .border_t_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .when(opens, |foot| foot.child(crate::t!(Key::KeyOpen)))
            .when(chosen, |foot| {
                foot.child(crate::t!(Key::KeyInsert)).child(crate::t!(Key::KeyPin))
            })
    }

    /// Chooses the probe's theme by name, for screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn choose_theme_named(&mut self, name: &str, cx: &mut Context<Self>) {
        self.probe.choose_named(name, cx);
        cx.notify();
    }
}

impl Focusable for MaterialPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MaterialPanel {
    /// A new place's content fades in, sliding a little from the side it was reached from.
    fn arriving(&self, content: AnyElement) -> impl IntoElement + use<> {
        let (arrival, down) = self.arrival;
        let from = if down {
            tokens::OUTLINE_SLIDE
        } else {
            gpui::px(0.) - tokens::OUTLINE_SLIDE
        };
        div().relative().child(content).with_animation(
            ("arrive", arrival),
            Animation::new(tokens::FADE).with_easing(ease_out_quint()),
            move |body, shown| body.opacity(shown).left(from * (1. - shown)),
        )
    }
}

impl Render for MaterialPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.load(cx);
        let tokens = Tokens::get(cx).clone();
        let jumping = self.jumping(window, cx);
        let content = match (jumping, self.current_item(cx)) {
            (false, Some(item)) => self.item_view(&item, &tokens, cx),
            _ => self.list(window, &tokens, cx),
        };
        div()
            .id("material-panel")
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(|panel, _: &Up, window, cx| panel.step(false, window, cx)))
            .on_action(cx.listener(|panel, _: &Down, window, cx| panel.step(true, window, cx)))
            .on_action(cx.listener(|panel, _: &input::Up, window, cx| panel.step(false, window, cx)))
            .on_action(cx.listener(|panel, _: &input::Down, window, cx| panel.step(true, window, cx)))
            .on_action(cx.listener(|panel, _: &Open, window, cx| panel.open(window, cx)))
            .on_action(cx.listener(|panel, _: &Insert, window, cx| panel.insert(window, cx)))
            .on_action(cx.listener(|panel, _: &Pin, window, cx| panel.pin(window, cx)))
            .on_action(cx.listener(|panel, _: &input::Commit, window, cx| panel.pin(window, cx)))
            .on_action(cx.listener(|panel, _: &Back, _, cx| panel.back(cx)))
            .on_action(cx.listener(|panel, _: &Forward, _, cx| panel.forward(cx)))
            .on_action(cx.listener(|panel, _: &Leave, window, cx| panel.leave(window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .child(self.header(&tokens, cx))
            .child(self.field_row(jumping, &tokens))
            .child(
                div()
                    .id("material-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .border_t_1()
                    .border_color(tokens.divider)
                    .child(self.arriving(content)),
            )
            .child(self.foot(window, &tokens, cx))
    }
}
