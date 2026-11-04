//! The Selection tab: the block under the caret, its alignment, and its named style. There are no per-block attributes, so
//! a change of alignment picks or makes a named style, kept in the theme file, and the block names it with `{.name}`.

use gpui::Focusable;
use gpui::prelude::*;
use gpui::{Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, svg};
use serde_json::{Value, json};

use crate::a11y::Control as _;
use crate::editor::rich::Key;
use crate::editor::rich::styles::{self, Class};
use crate::editor::rich::view::RichChanged;
use crate::editor::{CaretMoved, EditorView, SessionChanged, TextChanged};
use crate::inspector::input::{Cancelled, Submitted, TextInput};
use crate::inspector::{InspectorView, Tab, ThemeChanged, ThemeModel, ThemeSwitched};
use crate::source::BlockKind;
use crate::theme_file::editor::{Change, Outcome};
use crate::tokens::{self, Tokens};

/// What the tab shows, so a key elsewhere in the block draws nothing here.
#[derive(Clone, PartialEq, Eq, Default)]
struct Shown {
    title: String,
    class: Option<Class>,
    /// The built-in style the block starts from.
    base: &'static str,
    style: Option<String>,
    alignment: Option<String>,
    /// The theme's custom styles this kind of block may use.
    styles: Vec<String>,
}

pub struct SelectionPanel {
    editor: Entity<EditorView>,
    model: Entity<ThemeModel>,
    inspector: Entity<InspectorView>,
    shown: Shown,
    creating: Option<Entity<TextInput>>,
    /// A style waiting for the question about a common theme, with what to give the block once it is there.
    waiting: Option<String>,
    /// A style the theme now has, to give the block at the next draw, where a window is at hand.
    pending_apply: Option<String>,
    message: Option<String>,
    _subscriptions: Vec<Subscription>,
    _input_subscriptions: Vec<Subscription>,
}

const ALIGNMENTS: [(&str, &str, Key); 4] = [
    ("left", "icons/align-left.svg", Key::AlignLeft),
    ("center", "icons/align-center.svg", Key::AlignCenter),
    ("right", "icons/align-right.svg", Key::AlignRight),
    ("justify", "icons/align-justify.svg", Key::AlignJustify),
];

impl SelectionPanel {
    pub fn new(
        editor: Entity<EditorView>,
        model: Entity<ThemeModel>,
        inspector: Entity<InspectorView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.subscribe(&editor, |panel, _, _: &CaretMoved, cx| panel.sync(cx)),
            cx.subscribe(&editor, |panel, _, _: &TextChanged, cx| panel.sync(cx)),
            cx.subscribe(&editor, |panel, _, _: &SessionChanged, cx| panel.sync(cx)),
            cx.subscribe(&editor, |panel, _, _: &RichChanged, cx| panel.sync(cx)),
            cx.subscribe(&model, |panel, _, _: &ThemeChanged, cx| panel.theme_changed(cx)),
            cx.subscribe(&model, |panel, _, _: &ThemeSwitched, cx| panel.theme_changed(cx)),
        ];
        let mut panel = Self {
            editor,
            model,
            inspector,
            shown: Shown::default(),
            creating: None,
            waiting: None,
            pending_apply: None,
            message: None,
            _subscriptions: subscriptions,
            _input_subscriptions: Vec::new(),
        };
        panel.sync(cx);
        panel
    }

    fn merged<'a>(&self, cx: &'a gpui::App) -> Option<&'a Value> {
        self.model.read(cx).file().ok().map(|file| file.merged())
    }

    fn theme_changed(&mut self, cx: &mut Context<Self>) {
        self.sync(cx);
        // A style the question about a common theme held back is there now.
        if let Some(name) = self.waiting.clone()
            && self.shown.styles.contains(&name)
        {
            self.waiting = None;
            self.pending_apply = Some(name);
            cx.notify();
        }
    }

    /// Reads the block under the caret again. Draws only when something the tab shows changed.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let info = self.editor.read(cx).block_info();
        let mut shown = Shown::default();
        if let Some(info) = info {
            let merged = self.merged(cx);
            shown.title = title_of(info.kind, info.is_figure);
            shown.class = info.class;
            shown.base = styles::base_style(info.kind);
            shown.style = info.style.clone();
            if let Some(merged) = merged {
                shown.alignment = styles::alignment(merged, info.style.as_deref().unwrap_or(shown.base));
                if let Some(class) = info.class {
                    shown.styles = styles::styles_for(class, merged);
                }
            }
        }
        if shown != self.shown {
            self.shown = shown;
            cx.notify();
        }
    }

    fn pick(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .editor
            .update(cx, |editor, cx| editor.set_block_style(name.as_deref(), window, cx));
        self.message = result.err();
        self.sync(cx);
        cx.notify();
    }

    /// Writes a new style to the theme file, then gives it to the block.
    fn make_style(&mut self, name: String, style: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.read(cx).file().is_err() {
            self.message = Some(crate::t!(Key::NoTheme).to_owned());
            cx.notify();
            return;
        }
        let change = Change::set(&["custom-styles", &name], style);
        match self.model.update(cx, |model, cx| model.change(change, cx)) {
            Ok(Outcome::Saved) => {
                self.sync(cx);
                self.pick(Some(name), window, cx);
            }
            Ok(Outcome::NeedsDecision) => {
                self.waiting = Some(name);
                self.message = Some(crate::t!(Key::StyleWaits).to_owned());
            }
            Err(why) => self.message = Some(why),
        }
        cx.notify();
    }

    /// Picks or makes the named style that sets this block's text this way.
    fn set_alignment(&mut self, align: &str, window: &mut Window, cx: &mut Context<Self>) {
        let from = self.shown.style.clone();
        let base = from
            .as_deref()
            .map_or(self.shown.base, |name| styles::unaligned(name).unwrap_or(name))
            .to_owned();
        let name = styles::aligned_name(&base, align);
        if self.shown.styles.contains(&name) {
            self.pick(Some(name), window, cx);
        } else {
            self.make_style(name, json!({"based-on": base, "align": align}), window, cx);
        }
    }

    /// Takes the alignment style off, back to the style it came from, or to the theme's own.
    fn reset_alignment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let back = self
            .shown
            .style
            .as_deref()
            .and_then(styles::unaligned)
            .filter(|name| self.shown.styles.iter().any(|style| style == name))
            .map(str::to_owned);
        self.pick(back, window, cx);
    }

    fn start_creating(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .keep_focus_on_enter()
                .with_placeholder(crate::t!(Key::NewStylePlaceholder))
        });
        self._input_subscriptions = vec![
            cx.subscribe_in(&input, window, |panel, input, _: &Submitted, window, cx| {
                let name = input.read(cx).text().trim().to_owned();
                panel.create(name, window, cx);
            }),
            cx.subscribe_in(&input, window, |panel, _, _: &Cancelled, _, cx| {
                panel.creating = None;
                cx.notify();
            }),
        ];
        input.focus_handle(cx).focus(window, cx);
        self.creating = Some(input);
        cx.notify();
    }

    fn create(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if !styles::is_valid_name(&name) {
            self.message = Some(crate::t!(Key::StyleNameInvalid).to_owned());
            cx.notify();
            return;
        }
        let exists = self
            .merged(cx)
            .and_then(|merged| merged.get("custom-styles"))
            .is_some_and(|custom| custom.get(&name).is_some());
        if exists {
            self.message = Some(crate::t!(Key::StyleExists, name = name));
            cx.notify();
            return;
        }
        self.creating = None;
        let base = self.shown.style.clone().unwrap_or_else(|| self.shown.base.to_owned());
        self.make_style(name, json!({"based-on": base}), window, cx);
    }

    fn apply_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(name) = self.pending_apply.take() {
            self.pick(Some(name), window, cx);
        }
    }
}

fn title_of(kind: BlockKind, is_figure: bool) -> String {
    use crate::t;
    if is_figure {
        return t!(Key::BlockFigure).to_owned();
    }
    match kind {
        BlockKind::Paragraph => t!(Key::BlockParagraph).to_owned(),
        BlockKind::Heading { level } => t!(Key::BlockHeading, level = level),
        BlockKind::List => t!(Key::BlockList).to_owned(),
        BlockKind::Quote => t!(Key::BlockQuote).to_owned(),
        BlockKind::Code => t!(Key::BlockCode).to_owned(),
        BlockKind::Table => t!(Key::BlockTable).to_owned(),
        BlockKind::FootnoteDefinition => t!(Key::BlockFootnote).to_owned(),
        _ => t!(Key::BlockOther).to_owned(),
    }
}

impl Render for SelectionPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.apply_pending(window, cx);
        let tokens = Tokens::get(cx).clone();
        let shown = self.shown.clone();
        let section = |title: SharedString| {
            div()
                .flex_none()
                .h(tokens::SECTION_HEIGHT)
                .px(tokens::SPACE_3)
                .flex()
                .items_center()
                .border_b_1()
                .border_color(tokens.divider)
                .text_size(tokens::TEXT_SMALL)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.text_quiet)
                .child(title)
        };
        let root = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .text_size(tokens::TEXT_BODY);
        if shown.title.is_empty() {
            return root.child(section(crate::t!(Key::BlockOther).into())).child(
                div()
                    .p(tokens::SPACE_3)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(crate::t!(Key::SelectionEmpty)),
            );
        }
        let root = root.child(section(shown.title.clone().into()));
        if shown.class.is_none() {
            return root.child(
                div()
                    .p(tokens::SPACE_3)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_faint)
                    .child(crate::t!(Key::SelectionNoStyle)),
            );
        }
        let current_align = shown.alignment.clone().unwrap_or_default();
        let has_alignment_style = shown.style.as_deref().and_then(styles::unaligned).is_some();
        let mut segments = div()
            .w(px(156.))
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded(tokens::RADIUS)
            .border_1()
            .border_color(tokens.divider)
            .child(
                segment(
                    &tokens,
                    "align-theme",
                    "icons/align-default.svg",
                    crate::t!(Key::AlignDefault),
                    !has_alignment_style,
                )
                .on_click(cx.listener(|panel, _, window, cx| panel.reset_alignment(window, cx))),
            );
        for (word, icon, key) in ALIGNMENTS {
            let chosen = has_alignment_style && current_align == word;
            segments = segments.child(
                segment(&tokens, word, icon, crate::t!(key), chosen)
                    .on_click(cx.listener(move |panel, _, window, cx| panel.set_alignment(word, window, cx))),
            );
        }
        let alignment = div()
            .flex_none()
            .px(tokens::SPACE_3)
            .pb(tokens::SPACE_3)
            .border_b_1()
            .border_color(tokens.divider)
            .child(
                div()
                    .h(tokens::ROW_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child(crate::t!(Key::TextAlignment))
                    .when(has_alignment_style, |row| {
                        row.child(
                            div()
                                .id("align-reset")
                                .button(&tokens, crate::t!(Key::ResetLabel))
                                .cursor_pointer()
                                .text_color(tokens.accent)
                                .on_click(cx.listener(|panel, _, window, cx| panel.reset_alignment(window, cx)))
                                .child(crate::t!(Key::ResetLabel)),
                        )
                    }),
            )
            .child(segments);
        let style_row = |id: SharedString, label: SharedString, chosen: bool| {
            div()
                .id(id)
                .flex_none()
                .h(tokens::LIST_ROW_HEIGHT)
                .px(tokens::SPACE_3)
                .flex()
                .items_center()
                .justify_between()
                .cursor_pointer()
                .text_color(if chosen { tokens.text } else { tokens.text_quiet })
                .when(chosen, |row| row.bg(tokens.raised))
                .hover(|row| row.bg(tokens.raised))
                .child(div().min_w_0().truncate().child(label))
        };
        let mut list = div().flex().flex_col().child(
            style_row(
                "style-none".into(),
                crate::t!(Key::StyleDefault).into(),
                shown.style.is_none(),
            )
            .on_click(cx.listener(|panel, _, window, cx| panel.pick(None, window, cx))),
        );
        for name in &shown.styles {
            let chosen = shown.style.as_deref() == Some(name);
            let picked = name.clone();
            list = list.child(
                style_row(format!("style-{name}").into(), format!(".{name}").into(), chosen)
                    .on_click(cx.listener(move |panel, _, window, cx| panel.pick(Some(picked.clone()), window, cx))),
            );
        }
        let actions = div()
            .flex_none()
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .children(self.creating.clone())
            .child(
                div()
                    .flex()
                    .gap(tokens::SPACE_2)
                    .child(
                        crate::inspector::widgets::button(&tokens, "style-new", crate::t!(Key::NewStyle))
                            .on_click(cx.listener(|panel, _, window, cx| panel.start_creating(window, cx))),
                    )
                    .child(
                        div()
                            .id("style-edit")
                            .flex_none()
                            .size(tokens::FIELD_HEIGHT)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(tokens::RADIUS)
                            .border_1()
                            .border_color(tokens.divider)
                            .cursor_pointer()
                            .hover(|button| button.bg(tokens.raised))
                            .tooltip(crate::editor::float::tooltip(crate::t!(Key::EditStyle).to_owned()))
                            .on_click(cx.listener(|panel, _, _, cx| {
                                panel
                                    .inspector
                                    .update(cx, |inspector, cx| inspector.show(Tab::Styles, cx));
                            }))
                            .child(
                                svg()
                                    .path("icons/pencil.svg")
                                    .size(tokens::ICON_SIZE)
                                    .text_color(tokens.text_quiet),
                            ),
                    ),
            )
            .children(self.message.as_ref().map(|message| {
                div()
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.accent)
                    .child(message.clone())
            }));
        root.child(alignment)
            .child(section(crate::t!(Key::StyleSection).into()))
            .child(list)
            .child(actions)
    }
}

fn segment(
    tokens: &Tokens,
    id: &'static str,
    icon: &'static str,
    tip: &'static str,
    chosen: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .toggle(tokens, tip, chosen)
        .flex_none()
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
        .when(chosen, |button| button.bg(tokens.text))
        .when(!chosen, |button| button.hover(|button| button.bg(tokens.raised)))
        .tooltip(crate::editor::float::tooltip(tip.to_owned()))
        .child(
            svg()
                .path(icon)
                .size(tokens::ICON_SIZE)
                .text_color(if chosen { tokens.panel } else { tokens.text }),
        )
}
