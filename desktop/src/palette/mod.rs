//! The palette: `Cmd+P` finds a chapter and `Cmd+Shift+P` runs a command. One card at the top centre of the window, with a
//! field and a list. Commands come from [`crate::shell::commands`], so everything the menus and chrome offer is here, and
//! the keys stay with the text underneath only while the card is closed.

mod filter;
mod strings;

use gpui::prelude::*;
use gpui::{
    Action, App, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseDownEvent, ScrollHandle, Subscription,
    WeakEntity, Window, actions, div,
};

use self::strings::Key;
use crate::a11y::Control as _;
use crate::inspector::input::{self, Cancelled, Edited, Submitted, TextInput};
use crate::session::{self, ChapterId};
use crate::shell::commands::{self, Scope};
use crate::shell::{self, menus};
use crate::sidebar;
use crate::tokens::{self, Tokens};
use crate::workspace::{Current, Mode, Parts, Slots, Workspace};

actions!(palette, [FindChapter, RunCommand]);

/// Binds the keys. Once, when the app starts.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-p", FindChapter, None),
        KeyBinding::new("cmd-shift-p", RunCommand, None),
    ]);
    cx.on_action(|_: &FindChapter, cx| open(Kind::Chapters, cx));
    cx.on_action(|_: &RunCommand, cx| open(Kind::Commands, cx));
}

fn open(kind: Kind, cx: &mut App) {
    Current::<Palette>::defer(cx, move |palette, window, cx| palette.open(kind, window, cx));
}

/// Mounts the palette as a layer over the window.
pub fn mount(parts: &Parts, slots: &mut Slots, window: &mut Window, cx: &mut Context<Workspace>) {
    let workspace = cx.entity().downgrade();
    let palette = cx.new(|cx| Palette::new(parts.clone(), workspace, window, cx));
    Current::set(&palette, window, cx);
    slots.overlays.push(palette.into());
}

/// `DRUCK_PALETTE=chapters` or `commands` opens it, for screenshots.
#[cfg(any(debug_assertions, feature = "measure"))]
pub fn apply_gate(window: &mut Window, cx: &mut App) {
    let Some(name) = crate::gate::var("DRUCK_PALETTE") else {
        return;
    };
    let Some(palette) = Current::<Palette>::view(cx) else {
        return;
    };
    let kind = if name == "commands" {
        Kind::Commands
    } else {
        Kind::Chapters
    };
    palette.update(cx, |palette, cx| palette.open(kind, window, cx));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Chapters,
    Commands,
}

/// What a row does when chosen.
enum Target {
    Chapter(ChapterId),
    Command(Box<dyn Action>),
}

struct Row {
    label: String,
    /// The key that does the same, in the platform's symbols.
    keys: Option<String>,
    /// How a chapter's row starts, as in the Binder.
    chapter: Option<(session::Row, bool)>,
    target: Target,
}

pub struct Palette {
    parts: Parts,
    workspace: WeakEntity<Workspace>,
    kind: Option<Kind>,
    input: Entity<TextInput>,
    rows: Vec<Row>,
    /// What the field leaves of `rows`, best first.
    shown: Vec<usize>,
    selected: usize,
    /// What had the keys when the card opened, which get them back.
    before: Option<FocusHandle>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Palette {
    fn new(parts: Parts, workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .arrows_to_parent()
                .with_label(Key::Commands)
        });
        let subscriptions = vec![
            cx.subscribe(&input, |palette, _, _: &Edited, cx| palette.filter(cx)),
            cx.subscribe_in(&input, window, |palette, _, _: &Submitted, window, cx| {
                palette.accept(window, cx);
            }),
            cx.subscribe_in(&input, window, |palette, _, _: &Cancelled, window, cx| {
                palette.close(window, cx);
            }),
        ];
        Self {
            parts,
            workspace,
            kind: None,
            input,
            rows: Vec::new(),
            shown: Vec::new(),
            selected: 0,
            before: None,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn mode(&self, cx: &App) -> Mode {
        self.workspace
            .upgrade()
            .map_or(Mode::Write, |workspace| workspace.read(cx).mode())
    }

    /// Opens the card on a kind, or closes it if that kind is open already.
    fn open(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.kind == Some(kind) {
            self.close(window, cx);
            return;
        }
        if self.kind.is_none() {
            self.before = window.focused(cx);
        }
        self.kind = Some(kind);
        self.rows = match kind {
            Kind::Chapters => self.chapter_rows(cx),
            Kind::Commands => self.command_rows(cx),
        };
        let placeholder = crate::t!(match kind {
            Kind::Chapters => Key::Chapters,
            Kind::Commands => Key::Commands,
        });
        self.input.update(cx, |input, cx| {
            input.set_value(String::new(), true, cx);
            input.set_placeholder(placeholder, cx);
        });
        self.input.read(cx).focus_handle(cx).focus(window, cx);
        self.filter(cx);
        self.select_active_chapter(cx);
        cx.notify();
    }

    /// Closes the card and gives the keys back.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.kind = None;
        self.rows.clear();
        self.shown.clear();
        match self.before.take() {
            Some(before) => window.focus(&before, cx),
            None => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| workspace.focus_main(window, cx));
                }
            }
        }
        cx.notify();
    }

    fn chapter_rows(&self, cx: &App) -> Vec<Row> {
        let contents = self.parts.editor.read(cx).session().contents();
        let in_book = contents.book.iter().map(|row| (row, false));
        let files = (contents.not_in_book.iter())
            .chain(&contents.notes)
            .chain(&contents.research)
            .map(|row| (row, true));
        in_book
            .chain(files)
            .map(|(row, plain_file)| Row {
                label: row.title.clone(),
                keys: None,
                chapter: Some((row.clone(), plain_file)),
                target: Target::Chapter(row.id),
            })
            .collect()
    }

    fn command_rows(&self, cx: &App) -> Vec<Row> {
        let mode = self.mode(cx);
        let mut rows: Vec<Row> = commands::all()
            .into_iter()
            .filter(|command| command.is_available(cx) && (command.scope != Scope::Write || mode == Mode::Write))
            .map(|command| Row {
                label: command.name().to_owned(),
                keys: commands::keys_of(command.action.as_ref(), cx),
                chapter: None,
                target: Target::Command(command.action),
            })
            .collect();
        for (index, path) in shell::recent_documents(cx).iter().enumerate() {
            rows.push(Row {
                label: crate::t!(Key::OpenRecent, name = menus::recent_name(path)),
                keys: None,
                chapter: None,
                target: Target::Command(menus::recent_action(index)),
            });
        }
        rows
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().trim().to_owned();
        self.shown = filter::ranked(&query, self.rows.iter().map(|row| row.label.as_str()));
        self.selected = 0;
        self.scroll.scroll_to_item(0);
        cx.notify();
    }

    /// With the field empty, a chapter's list opens on the chapter in the text.
    fn select_active_chapter(&mut self, cx: &App) {
        let active = self.parts.editor.read(cx).session().active();
        let at = self
            .shown
            .iter()
            .position(|index| matches!(self.rows[*index].target, Target::Chapter(id) if id == active));
        if let Some(at) = at {
            self.selected = at;
            self.scroll.scroll_to_item(at);
        }
    }

    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(last) = self.shown.len().checked_sub(1) else {
            return;
        };
        self.selected = match (down, self.selected) {
            (true, at) if at >= last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(self.selected, window, cx);
    }

    /// Does what a row says, once the keys are back where they were.
    fn choose(&mut self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.shown.get(at).copied() else {
            return;
        };
        let target = self.rows.swap_remove(index).target;
        self.close(window, cx);
        match target {
            Target::Chapter(id) => {
                self.parts
                    .editor
                    .update(cx, |editor, cx| editor.show_chapter(id, window, cx));
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| workspace.focus_main(window, cx));
                }
            }
            Target::Command(action) => window.dispatch_action(action, cx),
        }
    }

    fn row(&self, at: usize, row: &Row, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let selected = at == self.selected;
        div()
            .id(("palette-row", at))
            .button(tokens, row.label.clone())
            .flex_none()
            .h(tokens::MENU_ROW_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .cursor_pointer()
            .text_color(if selected { tokens.text } else { tokens.text_quiet })
            .when(selected, |line| line.bg(tokens.raised))
            .hover(|line| line.bg(tokens.raised))
            .on_click(cx.listener(move |palette, _, window, cx| palette.choose(at, window, cx)))
            .when_some(row.chapter.as_ref(), |line, (chapter, plain_file)| {
                line.child(sidebar::row_lead(tokens, chapter, *plain_file, selected))
            })
            .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
            .when_some(row.keys.clone(), |line, keys| {
                line.child(
                    div()
                        .flex_none()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_faint)
                        .child(keys),
                )
            })
    }
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.kind.is_none() {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let rows: Vec<_> = (self.shown.iter().enumerate())
            .map(|(at, index)| self.row(at, &self.rows[*index], &tokens, cx).into_any_element())
            .collect();
        let empty = rows.is_empty();
        div()
            .id("palette")
            .occlude()
            .absolute()
            .top(tokens::PALETTE_TOP)
            .left(crate::workspace::TextColumn::centred(cx, tokens::PALETTE_WIDTH))
            .w(tokens::PALETTE_WIDTH)
            .flex()
            .flex_col()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow(tokens::card_shadow())
            .key_context("Palette")
            .on_action(cx.listener(|palette, _: &input::Up, _, cx| palette.step(false, cx)))
            .on_action(cx.listener(|palette, _: &input::Down, _, cx| palette.step(true, cx)))
            .on_mouse_down_out(cx.listener(|palette, _: &MouseDownEvent, window, cx| palette.close(window, cx)))
            .child(
                div()
                    .flex_none()
                    .p(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .child(self.input.clone()),
            )
            .child(
                div()
                    .id("palette-rows")
                    .max_h(tokens::MENU_ROW_HEIGHT * tokens::PALETTE_MAX_ROWS as f32)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .flex()
                    .flex_col()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .p(tokens::SPACE_3)
                                .text_color(tokens.text_faint)
                                .child(crate::t!(Key::NoMatches)),
                        )
                    }),
            )
            .into_any_element()
    }
}
