//! The theme picker at the top of the Styles tab: the project's themes and the common ones, a switch for the bundled specimen,
//! and create, rename, duplicate, and delete for common themes. Picking writes `theme:` to `druck.yaml` or the front
//! matter, and the pages and the inspector follow the document from there.

use gpui::prelude::*;
use gpui::{
    Anchor, Context, Entity, Focusable, Hsla, MouseButton, PromptLevel, ScrollHandle, SharedString, Subscription,
    Window, anchored, deferred, div, linear_color_stop, linear_gradient,
};

use super::input::{Submitted, TextInput};
use super::model::{ThemeModel, ThemeSwitched};
use super::strings::Key;
use super::widgets;
use crate::editor::EditorView;
use crate::preview::PreviewView;
use crate::session::Field;
use crate::theme_file::library::{self, Entry, Scope};
use crate::tokens::{self, Tokens};

/// What the name field in the menu is for.
enum Naming {
    Create,
    Rename(String),
}

pub struct ThemePicker {
    editor: Entity<EditorView>,
    preview: Entity<PreviewView>,
    model: Entity<ThemeModel>,
    open: bool,
    /// The common theme whose actions are showing.
    expanded: Option<String>,
    naming: Option<(Naming, Entity<TextInput>)>,
    error: Option<String>,
    /// The menu's list, which fades at its foot while more is below.
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ThemePicker {
    pub fn new(
        editor: Entity<EditorView>,
        preview: Entity<PreviewView>,
        model: Entity<ThemeModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe(&model, |_, _, _: &ThemeSwitched, cx| cx.notify()),
            cx.observe(&preview, |_, _, cx| cx.notify()),
        ];
        Self {
            editor,
            preview,
            model,
            open: false,
            expanded: None,
            scroll: ScrollHandle::new(),
            naming: None,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn common(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        self.model
            .read(cx)
            .editor()
            .dirs()
            .common
            .clone()
            .or_else(library::common_dir)
    }

    fn project(&self, cx: &Context<Self>) -> Option<std::path::PathBuf> {
        self.model.read(cx).editor().dirs().project.clone()
    }

    /// Opens the menu, as a click on the button does.
    pub fn open_menu(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        cx.notify();
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.expanded = None;
        self.naming = None;
        self.error = None;
        cx.notify();
    }

    /// Makes the document use a theme, or the bundled default for `None`.
    fn pick(&mut self, name: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        let name = name.unwrap_or_default().to_owned();
        let result = self.editor.update(cx, |editor, cx| {
            editor.update_session(|session| session.set_field(Field::Theme, &name), window, cx)
        });
        match result {
            Ok(()) => self.close(cx),
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    fn start_naming(&mut self, naming: Naming, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |input, cx| input.set_value(text, true, cx));
        self._subscriptions.push(
            cx.subscribe_in(&input, window, |picker, input, _: &Submitted, window, cx| {
                let name = input.read(cx).text().trim().to_owned();
                picker.finish_naming(&name, window, cx);
            }),
        );
        input.read(cx).focus_handle(cx).focus(window, cx);
        self.naming = Some((naming, input));
        self.error = None;
        cx.notify();
    }

    fn finish_naming(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((naming, _)) = self.naming.take() else { return };
        let Some(common) = self.common(cx) else { return };
        let result = match &naming {
            Naming::Create => library::create(&common, name).map(|_| ()),
            Naming::Rename(from) => library::rename(&common, from, name).map(|_| ()),
        };
        if let Err(message) = result {
            self.error = Some(message);
            cx.notify();
            return;
        }
        match naming {
            Naming::Create => self.pick(Some(name), window, cx),
            Naming::Rename(from) => {
                let used = self.editor.read(cx).session().theme_setting().as_deref() == Some(from.as_str());
                if used {
                    self.pick(Some(name), window, cx);
                } else {
                    self.close(cx);
                }
            }
        }
    }

    fn duplicate(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(common) = self.common(cx) else { return };
        let copy = library::free_copy_name(&common, name);
        match library::duplicate(&common, name, &copy) {
            Ok(_) => self.pick(Some(&copy), window, cx),
            Err(message) => {
                self.error = Some(message);
                cx.notify();
            }
        }
    }

    /// Asks before deleting, since a deleted theme cannot come back and documents may name it.
    fn delete(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(common) = self.common(cx) else { return };
        let used = self.editor.read(cx).session().theme_setting().as_deref() == Some(name);
        let mut detail = crate::t!(Key::DeleteThemeDetail).to_owned();
        if used {
            detail = format!("{} {detail}", crate::t!(Key::DeleteThemeOpen));
        }
        let answer = window.prompt(
            PromptLevel::Warning,
            &crate::t!(Key::DeleteTheme, value = name),
            Some(&detail),
            &[crate::t!(Key::Delete), crate::t!(Key::Cancel)],
            cx,
        );
        let name = name.to_owned();
        cx.spawn_in(window, async move |picker, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            picker
                .update_in(cx, |picker, window, cx| match library::delete(&common, &name) {
                    Ok(()) => {
                        if picker.editor.read(cx).session().theme_setting().as_deref() == Some(name.as_str()) {
                            picker.pick(None, window, cx);
                        } else {
                            picker.close(cx);
                        }
                    }
                    Err(message) => {
                        picker.error = Some(message);
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
    }

    fn entry_row(
        &self,
        entry: &Entry,
        current: Option<&str>,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let name = entry.name.clone();
        let active = current == Some(entry.name.as_str());
        let common = entry.scope == Scope::Common;
        let expanded = common && self.expanded.as_deref() == Some(entry.name.as_str());
        let id = |what: &str| SharedString::from(format!("{what}-{}", entry.name));
        let pick_name = name.clone();
        let mut row = widgets::menu_item(tokens, id("theme"), name.clone(), None, active)
            .on_click(cx.listener(move |picker, _, window, cx| picker.pick(Some(&pick_name), window, cx)));
        if common {
            let toggled = name.clone();
            row = row.child(
                widgets::quiet_link(tokens, id("more"), if expanded { "▴" } else { "⋯" }).on_click(cx.listener(
                    move |picker, _, _, cx| {
                        cx.stop_propagation();
                        picker.expanded = if picker.expanded.as_deref() == Some(toggled.as_str()) {
                            None
                        } else {
                            Some(toggled.clone())
                        };
                        cx.notify();
                    },
                )),
            );
        }
        let mut column = div().flex().flex_col().child(row);
        if expanded {
            let (renamed, copied, deleted) = (name.clone(), name.clone(), name.clone());
            column = column.child(
                div()
                    .px(tokens::SPACE_2)
                    .py(tokens::SPACE_1)
                    .flex()
                    .gap(tokens::SPACE_3)
                    .child(
                        widgets::quiet_link(tokens, id("rename"), Key::Rename).on_click(cx.listener(
                            move |picker, _, window, cx| {
                                picker.start_naming(Naming::Rename(renamed.clone()), renamed.clone(), window, cx);
                            },
                        )),
                    )
                    .child(
                        widgets::quiet_link(tokens, id("duplicate"), Key::Duplicate)
                            .on_click(cx.listener(move |picker, _, window, cx| picker.duplicate(&copied, window, cx))),
                    )
                    .child(
                        widgets::quiet_link(tokens, id("delete"), Key::Delete)
                            .on_click(cx.listener(move |picker, _, window, cx| picker.delete(&deleted, window, cx))),
                    ),
            );
        }
        column.into_any_element()
    }

    fn menu(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let project = self.project(cx);
        let common = self.common(cx);
        let current = self.editor.read(cx).session().theme_setting();
        let label = |text: Key| {
            div()
                .px(tokens::SPACE_2)
                .pt(tokens::SPACE_2)
                .pb(tokens::SPACE_1)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(crate::t!(text))
        };
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        let default_active = current.is_none();
        rows.push(
            widgets::menu_item(
                tokens,
                "theme-default",
                crate::t!(Key::DefaultTheme),
                None,
                default_active,
            )
            .on_click(cx.listener(|picker, _, window, cx| picker.pick(None, window, cx)))
            .into_any_element(),
        );
        let project_entries: Vec<Entry> = library::list(project.as_deref(), None)
            .into_iter()
            .filter(|entry| entry.scope == Scope::Project)
            .collect();
        if !project_entries.is_empty() {
            rows.push(label(Key::ProjectThemes).into_any_element());
            for entry in &project_entries {
                rows.push(self.entry_row(entry, current.as_deref(), tokens, cx));
            }
        }
        if let Some(common) = &common {
            rows.push(label(Key::SharedThemes).into_any_element());
            for entry in library::list(None, Some(common)) {
                // A project theme of the same name wins, so the document means that one.
                let shadowed = project_entries.iter().any(|project| project.name == entry.name);
                rows.push(self.entry_row(&entry, current.as_deref().filter(|_| !shadowed), tokens, cx));
            }
            rows.push(match &self.naming {
                Some((_, input)) => div().p(tokens::SPACE_2).child(input.clone()).into_any_element(),
                None => widgets::quiet_link(tokens, "theme-new", format!("+ {}", crate::t!(Key::NewSharedTheme)))
                    .m(tokens::SPACE_2)
                    .on_click(cx.listener(|picker, _, window, cx| {
                        picker.start_naming(Naming::Create, String::new(), window, cx);
                    }))
                    .into_any_element(),
            });
        }
        if let Some(message) = &self.error {
            rows.push(
                div()
                    .px(tokens::SPACE_2)
                    .pb(tokens::SPACE_2)
                    .child(widgets::error_line(tokens, message.clone()))
                    .into_any_element(),
            );
        }
        let more_below = {
            let (offset, most) = (self.scroll.offset().y, self.scroll.max_offset().y);
            most > tokens::HAIRLINE && -offset < most
        };
        let (clear, solid) = (
            Hsla::from(tokens.panel).opacity(tokens::CLEAR),
            Hsla::from(tokens.panel),
        );
        div()
            .relative()
            .w(tokens::INSPECTOR_WIDTH)
            .mt(tokens::SPACE_1)
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow_lg()
            .overflow_hidden()
            .text_size(tokens::TEXT_BODY)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|picker, _, _, cx| picker.close(cx)))
            .child(
                div()
                    .id("theme-menu")
                    .occlude()
                    .max_h(tokens::DIALOG_LIST_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .py(tokens::SPACE_1)
                    .children(rows),
            )
            .when(more_below, |menu| {
                menu.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(tokens::VERSIONS_ROW_HEIGHT)
                        .bg(linear_gradient(
                            tokens::FADE_DOWN,
                            linear_color_stop(clear, 0.),
                            linear_color_stop(solid, 1.),
                        )),
                )
            })
    }
}

impl Render for ThemePicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let name = self
            .model
            .read(cx)
            .editor()
            .name()
            .unwrap_or_else(|| crate::t!(Key::DefaultTheme).to_owned());
        let specimen = self.preview.read(cx).is_specimen();
        let button = |id: &'static str, active: bool| {
            div()
                .id(id)
                .role(gpui::Role::Button)
                .focusable()
                .tab_stop(true)
                .px(tokens::SPACE_2)
                .rounded(tokens::RADIUS)
                .cursor_pointer()
                .text_color(if active { tokens.text } else { tokens.text_quiet })
                .when(active, |button| button.bg(tokens.raised))
                .hover(|button| button.bg(tokens.raised))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        div()
            .relative()
            .flex_none()
            .h(tokens::SECTION_HEIGHT)
            .px(tokens::SPACE_2)
            .flex()
            .items_center()
            .justify_between()
            .gap(tokens::SPACE_2)
            .border_b_1()
            .border_color(tokens.divider)
            .text_size(tokens::TEXT_SMALL)
            .child(
                button("theme-picker", self.open)
                    .on_click(cx.listener(|picker, _, _, cx| {
                        picker.open = !picker.open;
                        picker.expanded = None;
                        picker.naming = None;
                        picker.error = None;
                        cx.notify();
                    }))
                    .aria_label(format!("{}: {name}", crate::t!(Key::Theme)))
                    .min_w_0()
                    .truncate()
                    .child(format!("{}: {name} ▾", crate::t!(Key::Theme))),
            )
            .child(
                button("specimen", specimen)
                    .on_click(cx.listener(|picker, _, _, cx| {
                        let on = !picker.preview.read(cx).is_specimen();
                        picker.preview.update(cx, |preview, cx| preview.set_specimen(on, cx));
                    }))
                    .aria_label(crate::t!(Key::Specimen))
                    .child(crate::t!(Key::Specimen)),
            )
            .when(self.open, |bar| {
                // The menu hangs from the bar across the panel's width, so nothing of the index shows beside it.
                bar.child(div().absolute().top_full().left_0().child(deferred(
                    anchored().anchor(Anchor::TopLeft).child(self.menu(&tokens, cx)),
                )))
            })
    }
}
