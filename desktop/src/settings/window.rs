//! The settings window, after Zed's: categories on the left, then a search field, then rows. A row is a label with one
//! quiet line of description and its control. A choice applies at once.

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, FontWeight, Rgba, SharedString, Stateful, Subscription, TitlebarOptions,
    Window, WindowBounds, WindowKind, WindowOptions, div, point, px, rgb, size, svg,
};

use super::file::{Appearance, LineLength, Settings, hex, parse_hex};
use super::strings::Key;
use super::{Store, change, file_path, get, open_file, problem};
use crate::a11y::Control as _;
use crate::inspector::input::{Committed, Edited, TextInput};
use crate::shell::commands;
use crate::shell::menus::CloseDocument;
use crate::shell::strings::LanguageSetting;
use crate::tokens::{self, ACCENTS, Tokens};

/// A category of the left list, in its order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Writing,
    Language,
    Keys,
}

impl Category {
    const ALL: [Self; 4] = [Self::Appearance, Self::Writing, Self::Language, Self::Keys];

    fn label(self) -> Key {
        match self {
            Self::Appearance => Key::CategoryAppearance,
            Self::Writing => Key::CategoryWriting,
            Self::Language => Key::CategoryLanguage,
            Self::Keys => Key::CategoryKeys,
        }
    }

    /// The category `DRUCK_SETTINGS` names, for screenshots.
    fn from_name(name: &str) -> Self {
        match name {
            "writing" => Self::Writing,
            "language" => Self::Language,
            "keys" => Self::Keys,
            _ => Self::Appearance,
        }
    }
}

/// What a row's control sets.
#[derive(Clone, Copy)]
enum Control {
    Appearance,
    Accent,
    LineLength,
    Language,
    Switch(fn(&Settings) -> bool, fn(&mut Settings, bool)),
}

struct Row {
    category: Category,
    label: Key,
    hint: Key,
    control: Control,
}

const ROWS: &[Row] = &[
    Row {
        category: Category::Appearance,
        label: Key::Appearance,
        hint: Key::AppearanceHint,
        control: Control::Appearance,
    },
    Row {
        category: Category::Appearance,
        label: Key::Accent,
        hint: Key::AccentHint,
        control: Control::Accent,
    },
    Row {
        category: Category::Writing,
        label: Key::LineLength,
        hint: Key::LineLengthHint,
        control: Control::LineLength,
    },
    Row {
        category: Category::Writing,
        label: Key::Typewriter,
        hint: Key::TypewriterHint,
        control: Control::Switch(|s| s.typewriter, |s, on| s.typewriter = on),
    },
    Row {
        category: Category::Writing,
        label: Key::Dim,
        hint: Key::DimHint,
        control: Control::Switch(|s| s.dim_paragraphs, |s, on| s.dim_paragraphs = on),
    },
    Row {
        category: Category::Writing,
        label: Key::SelectionToolbar,
        hint: Key::SelectionToolbarHint,
        control: Control::Switch(|s| s.selection_toolbar, |s, on| s.selection_toolbar = on),
    },
    Row {
        category: Category::Writing,
        label: Key::Binder,
        hint: Key::BinderHint,
        control: Control::Switch(|s| s.binder, |s, on| s.binder = on),
    },
    Row {
        category: Category::Language,
        label: Key::Language,
        hint: Key::LanguageHint,
        control: Control::Language,
    },
    Row {
        category: Category::Language,
        label: Key::Spelling,
        hint: Key::SpellingHint,
        control: Control::Switch(|s| s.spelling, |s, on| s.spelling = on),
    },
];

/// The line lengths the window offers. The file takes any.
const LINE_CHOICES: [LineLength; 4] = [
    LineLength::Book,
    LineLength::Characters(60),
    LineLength::Characters(70),
    LineLength::Characters(80),
];

/// Whether a row holds what is searched for: every word somewhere in its label or description.
fn matches(query: &str, label: &str, hint: &str) -> bool {
    let (label, hint) = (label.to_lowercase(), hint.to_lowercase());
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| label.contains(word) || hint.contains(word))
}

/// The window's handle while it is open.
#[derive(Clone, Copy)]
struct Open(gpui::WindowHandle<SettingsWindow>);

impl gpui::Global for Open {}

/// Brings the settings window up, opening it if need be. `measuring` runs float it without taking focus.
pub fn open(category: Option<&str>, cx: &mut App) {
    if let Some(open) = cx.try_global::<Open>().copied()
        && open.0.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let measuring = crate::gate::var("DRUCK_GATE").is_some();
    let (width, height) = tokens::SETTINGS_WINDOW;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(width), px(height)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(crate::t!(Key::Title).into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(9.), px(7.))),
        }),
        focus: !measuring,
        kind: if measuring {
            WindowKind::PopUp
        } else {
            WindowKind::Normal
        },
        ..Default::default()
    };
    let category = category.map_or(Category::Appearance, Category::from_name);
    let opened = cx.open_window(options, |window, cx| {
        cx.new(|cx| SettingsWindow::new(category, window, cx))
    });
    if let Ok(handle) = opened {
        cx.set_global(Open(handle));
    }
}

pub struct SettingsWindow {
    category: Category,
    focus: FocusHandle,
    search: Entity<TextInput>,
    accent: Entity<TextInput>,
    /// The accent field holds something that is not a colour.
    accent_bad: bool,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(category: Category, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::inspector::input::bind_keys(cx);
        let search = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_label(crate::t!(Key::Search))
                .with_placeholder(crate::t!(Key::Search))
        });
        let accent = cx.new(|cx| TextInput::new(window, cx).with_label(crate::t!(Key::Accent)));
        let subscriptions = vec![
            cx.observe_global_in::<Store>(window, |view, window, cx| view.settings_changed(window, cx)),
            cx.observe_global_in::<Tokens>(window, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, _: &Edited, cx| cx.notify()),
            cx.subscribe(&accent, |view, _, Committed(text), cx| view.accent_typed(text, cx)),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut view = Self {
            category,
            focus,
            search,
            accent,
            accent_bad: false,
            _subscriptions: subscriptions,
        };
        view.show_accent(cx);
        view
    }

    fn settings_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_accent(cx);
        window.set_window_title(crate::t!(Key::Title));
        self.search
            .update(cx, |search, cx| search.set_placeholder(crate::t!(Key::Search), cx));
        cx.notify();
    }

    /// Puts the accent in use in its field.
    fn show_accent(&mut self, cx: &mut Context<Self>) {
        let shown = hex(rgba_u32(Tokens::get(cx).accent));
        let same = self.accent.read(cx).text() == shown;
        if !same {
            self.accent.update(cx, |field, cx| field.set_value(shown, true, cx));
        }
        self.accent_bad = false;
    }

    fn accent_typed(&mut self, text: &str, cx: &mut Context<Self>) {
        match parse_hex(text) {
            Some(color) => change(cx, |settings| settings.set_accent(color)),
            None => self.accent_bad = !text.trim().is_empty(),
        }
        cx.notify();
    }

    fn query(&self, cx: &App) -> String {
        self.search.read(cx).text().trim().to_owned()
    }

    /// A row of segments, the chosen one raised.
    fn segments<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        choices: Vec<(T, String)>,
        chosen: T,
        set: fn(&mut Settings, T),
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<T> {
        let row = choices.into_iter().enumerate().map(|(at, (choice, label))| {
            let on = choice == chosen;
            div()
                .id((id, at))
                .toggle(tokens, label.clone(), on)
                .px(tokens::SPACE_2 + tokens::SPACE_1 / 2.)
                .h_full()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(tokens::TEXT_SMALL)
                .text_color(if on { tokens.text } else { tokens.text_quiet })
                .when(on, |segment| segment.bg(tokens.raised))
                .when(at > 0, |segment| segment.border_l_1().border_color(tokens.divider))
                .on_click(cx.listener(move |_, _, _, cx| change(cx, |settings| set(settings, choice))))
                .child(label)
        });
        div()
            .flex_none()
            .h(tokens::FIELD_HEIGHT - tokens::SPACE_1)
            .flex()
            .rounded(tokens::RADIUS)
            .overflow_hidden()
            .border_1()
            .border_color(tokens.divider)
            .bg(tokens.field)
            .children(row)
    }

    fn switch(
        &self,
        id: SharedString,
        on: bool,
        set: fn(&mut Settings, bool),
        label: &'static str,
        tokens: &Tokens,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        let inset = (tokens::SWITCH_HEIGHT - tokens::SWITCH_KNOB) / 2.;
        div()
            .id(id)
            .control(tokens, gpui::Role::Switch, label)
            .aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
            .flex_none()
            .relative()
            .w(tokens::SWITCH_WIDTH)
            .h(tokens::SWITCH_HEIGHT)
            .rounded(px(4.))
            .cursor_pointer()
            .bg(if on { tokens.accent } else { tokens.raised })
            .on_click(cx.listener(move |_, _, _, cx| change(cx, |settings| set(settings, !on))))
            .child(
                div()
                    .absolute()
                    .top(inset)
                    .when(on, |knob| knob.right(inset))
                    .when(!on, |knob| knob.left(inset))
                    .size(tokens::SWITCH_KNOB)
                    .rounded(px(3.))
                    .bg(gpui::white()),
            )
    }

    fn accent_control(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current = rgba_u32(tokens.accent);
        let chosen = get(cx).accent;
        let swatches = ACCENTS.into_iter().enumerate().map(|(at, color)| {
            // The first is the default, which the tokens may darken for a light surface.
            let shown = if at == 0 && chosen.is_none() { current } else { color };
            let on = shown == current;
            div()
                .id(("accent", at))
                .toggle(tokens, hex(color), on)
                .flex_none()
                .size(tokens::SETTINGS_SWATCH)
                .rounded(px(4.))
                .bg(rgb(shown))
                .cursor_pointer()
                .when(on, |swatch| swatch.border_2().border_color(tokens.text))
                .on_click(cx.listener(move |_, _, _, cx| change(cx, |settings| settings.set_accent(color))))
        });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .items_end()
            .gap(tokens::SPACE_1)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1 + px(1.))
                    .children(swatches)
                    .child(
                        div()
                            .flex_none()
                            .w(tokens::SETTINGS_HEX_WIDTH)
                            .ml(tokens::SPACE_1)
                            .child(self.accent.clone()),
                    ),
            )
            .when(self.accent_bad, |control| {
                control.child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.accent)
                        .child(crate::t!(Key::AccentBad)),
                )
            })
    }

    fn control(&self, row: &Row, tokens: &Tokens, cx: &mut Context<Self>) -> gpui::AnyElement {
        let settings = get(cx).clone();
        match row.control {
            Control::Appearance => self
                .segments(
                    "appearance",
                    vec![
                        (Appearance::System, crate::t!(Key::AppearanceSystem).to_owned()),
                        (Appearance::Light, crate::t!(Key::AppearanceLight).to_owned()),
                        (Appearance::Dark, crate::t!(Key::AppearanceDark).to_owned()),
                    ],
                    settings.appearance,
                    |settings, value| settings.appearance = value,
                    tokens,
                    cx,
                )
                .into_any_element(),
            Control::Accent => self.accent_control(tokens, cx).into_any_element(),
            Control::LineLength => {
                let mut choices: Vec<LineLength> = LINE_CHOICES.to_vec();
                // What the file says shows even if the window does not offer it.
                if !choices.contains(&settings.line_length) {
                    choices.push(settings.line_length);
                }
                let labelled = choices
                    .into_iter()
                    .map(|choice| {
                        let label = match choice {
                            LineLength::Book => crate::t!(Key::LineLengthBook).to_owned(),
                            LineLength::Characters(count) => count.to_string(),
                        };
                        (choice, label)
                    })
                    .collect();
                self.segments(
                    "line-length",
                    labelled,
                    settings.line_length,
                    |settings, value| settings.line_length = value,
                    tokens,
                    cx,
                )
                .into_any_element()
            }
            Control::Language => self
                .segments(
                    "language",
                    vec![
                        (LanguageSetting::System, crate::t!(Key::LanguageSystem).to_owned()),
                        (LanguageSetting::English, crate::t!(Key::LanguageEnglish).to_owned()),
                        (LanguageSetting::German, crate::t!(Key::LanguageGerman).to_owned()),
                    ],
                    settings.language,
                    |settings, value| settings.language = value,
                    tokens,
                    cx,
                )
                .into_any_element(),
            Control::Switch(read, set) => {
                let id: SharedString = format!("switch-{:?}", row.label).into();
                self.switch(id, read(&settings), set, crate::t!(row.label), tokens, cx)
                    .into_any_element()
            }
        }
    }

    /// A row: a label with one quiet line of description at the left, and what goes at the right, on a thin divider.
    fn row(
        label: impl Into<SharedString>,
        hint: Option<&'static str>,
        end: impl IntoElement,
        tokens: &Tokens,
    ) -> gpui::Div {
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(tokens::SPACE_4)
            .py(tokens::SPACE_2 + px(2.))
            .border_t_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(label.into())
                    .when_some(hint, |text, hint| {
                        text.child(
                            div()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_quiet)
                                .child(hint),
                        )
                    }),
            )
            .child(end)
    }

    fn group_label(label: &'static str, tokens: &Tokens) -> gpui::Div {
        div()
            .flex_none()
            .pt(tokens::SPACE_3)
            .pb(tokens::SPACE_2)
            .text_size(tokens::TEXT_SMALL)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(tokens.text_quiet)
            .child(label.to_uppercase())
    }

    /// The rows of one category, or what the search finds in all of them.
    fn rows(&self, query: &str, tokens: &Tokens, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let categories: Vec<Category> = if query.is_empty() {
            vec![self.category]
        } else {
            Category::ALL.to_vec()
        };
        let mut elements = Vec::new();
        for category in categories {
            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            if category == Category::Keys {
                for command in commands::all() {
                    let Some(keys) = commands::keys_of(command.action.as_ref(), cx) else {
                        continue;
                    };
                    if matches(query, crate::t!(command.label), &keys) {
                        rows.push(
                            Self::row(
                                crate::t!(command.label),
                                None,
                                div().flex_none().text_color(tokens.text_quiet).child(keys),
                                tokens,
                            )
                            .into_any_element(),
                        );
                    }
                }
            } else {
                for row in ROWS.iter().filter(|row| row.category == category) {
                    let (label, hint) = (crate::t!(row.label), crate::t!(row.hint));
                    if matches(query, label, hint) {
                        let end = self.control(row, tokens, cx);
                        rows.push(Self::row(label, Some(hint), end, tokens).into_any_element());
                    }
                }
            }
            if !rows.is_empty() {
                elements.push(Self::group_label(crate::t!(category.label()), tokens).into_any_element());
                elements.extend(rows);
            }
        }
        elements
    }
}

fn rgba_u32(color: Rgba) -> u32 {
    let byte = |channel: f32| (channel * 255.).round() as u32;
    byte(color.r) << 16 | byte(color.g) << 8 | byte(color.b)
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let query = self.query(cx);
        let rows = self.rows(&query, &tokens, cx);
        let nav: Vec<_> = Category::ALL
            .into_iter()
            .map(|category| {
                let on = query.is_empty() && category == self.category;
                let label = crate::t!(category.label());
                div()
                    .id(label)
                    .toggle(&tokens, label, on)
                    .flex_none()
                    .h(tokens::LIST_ROW_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_color(if on { tokens.text } else { tokens.text_quiet })
                    .when(on, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.category = category;
                        view.search
                            .update(cx, |search, cx| search.set_value(String::new(), true, cx));
                        window.focus(&view.focus, cx);
                        cx.notify();
                    }))
                    .child(label)
            })
            .collect();
        let link = div()
            .id("edit-settings-file")
            .button(&tokens, crate::t!(Key::EditFile))
            .flex_none()
            .h(tokens::LIST_ROW_HEIGHT)
            .px(tokens::SPACE_3)
            .flex()
            .items_center()
            .border_t_1()
            .border_color(tokens.divider)
            .cursor_pointer()
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_quiet)
            .hover(|row| row.text_color(tokens.text))
            .when_some(file_path(cx).map(|file| file.display().to_string()), |link, file| {
                link.tooltip(crate::editor::tooltip(file))
            })
            .on_click(|_, _, cx| open_file(cx))
            .child(crate::t!(Key::EditFile));
        let empty = rows.is_empty();
        let notice = problem(cx).map(|problem| crate::t!(Key::Invalid, problem = problem));
        div()
            .id("settings-window")
            .size_full()
            .track_focus(&self.focus)
            .on_action(|_: &CloseDocument, window, _| window.remove_window())
            .flex()
            .flex_col()
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .text_color(tokens.text)
            .bg(tokens.canvas)
            .child(
                div()
                    .flex_none()
                    .h(tokens::TITLEBAR_HEIGHT)
                    .pl(tokens::SPACE_4 * 5.)
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
                        if event.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(crate::t!(Key::Title)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .flex_none()
                            .w(tokens::SETTINGS_NAV_WIDTH)
                            .flex()
                            .flex_col()
                            .bg(tokens.panel)
                            .border_r_1()
                            .border_color(tokens.divider)
                            .pt(tokens::SPACE_2)
                            .children(nav)
                            .child(div().flex_1())
                            .child(link),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .px(tokens::SPACE_4 + tokens::SPACE_1)
                            .child(
                                div()
                                    .flex_none()
                                    .py(tokens::SPACE_3)
                                    .flex()
                                    .items_center()
                                    .gap(tokens::SPACE_2)
                                    .child(
                                        svg()
                                            .flex_none()
                                            .path("icons/search.svg")
                                            .size(tokens::ICON_SIZE)
                                            .text_color(tokens.text_faint),
                                    )
                                    .child(div().flex_1().min_w_0().child(self.search.clone())),
                            )
                            .when_some(notice, |body, notice| {
                                body.child(
                                    div()
                                        .flex_none()
                                        .pb(tokens::SPACE_2)
                                        .text_size(tokens::TEXT_SMALL)
                                        .text_color(tokens.accent)
                                        .child(notice),
                                )
                            })
                            .child(
                                div()
                                    .id("settings-rows")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .children(rows)
                                    .when(empty, |list| {
                                        list.child(
                                            div()
                                                .pt(tokens::SPACE_3)
                                                .text_color(tokens.text_faint)
                                                .child(crate::t!(Key::NoMatches)),
                                        )
                                    }),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_needs_every_word_somewhere_in_the_row() {
        assert!(matches("line", "Line length", "The book's measure"));
        assert!(matches("LENGTH book", "Line length", "The book's measure"));
        assert!(!matches("length colour", "Line length", "The book's measure"));
        assert!(matches("", "Line length", ""));
    }

    #[test]
    fn a_colour_goes_to_its_bytes_and_back() {
        let color = rgb(0x2f8fb8);
        assert_eq!(rgba_u32(color), 0x2f8fb8);
    }
}
