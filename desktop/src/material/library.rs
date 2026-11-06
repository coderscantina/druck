//! The library on `Cmd+Shift+O`: a card under the caret's paragraph that searches all material, filters it by kind,
//! previews the chosen item, and offers Pin here, the kind's insert, and Open in the rail, on the same keys as the panel.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight, KeyBinding, MouseDownEvent, ObjectFit, Role,
    ScrollHandle, SharedString, Subscription, Window, actions, div, img, px, svg,
};

use super::model::{Form, Item, Kind, Where};
use super::{Key, Material, MaterialChanged, icon, insert_label, kind_label};
use crate::editor::EditorView;
use crate::editor::review::small_button;
use crate::inspector::input::{self, Cancelled, Edited, Submitted, TextInput};
use crate::tokens::{self, Tokens};
use crate::workspace::TextColumn;

const CONTEXT: &str = "Library";

actions!(library, [InsertChosen, Filter1, Filter2, Filter3, Filter4, Filter5, Filter6, Filter7, Filter8]);

pub fn bind_keys(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("shift-enter", InsertChosen, context),
        KeyBinding::new("cmd-1", Filter1, context),
        KeyBinding::new("cmd-2", Filter2, context),
        KeyBinding::new("cmd-3", Filter3, context),
        KeyBinding::new("cmd-4", Filter4, context),
        KeyBinding::new("cmd-5", Filter5, context),
        KeyBinding::new("cmd-6", Filter6, context),
        KeyBinding::new("cmd-7", Filter7, context),
        KeyBinding::new("cmd-8", Filter8, context),
    ]);
}

/// A kind the library narrows to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    All,
    ThisChapter,
    Research,
    Notes,
    Probes,
    Images,
    Tables,
    Sources,
}

impl Filter {
    const ALL: [Self; 8] = [
        Self::All,
        Self::ThisChapter,
        Self::Research,
        Self::Notes,
        Self::Probes,
        Self::Images,
        Self::Tables,
        Self::Sources,
    ];

    fn label(self) -> &'static str {
        crate::t!(match self {
            Self::All => Key::All,
            Self::ThisChapter => Key::ThisChapter,
            Self::Research => Key::Research,
            Self::Notes => Key::Notes,
            Self::Probes => Key::Probes,
            Self::Images => Key::Images,
            Self::Tables => Key::Tables,
            Self::Sources => Key::Sources,
        })
    }

    /// Whether `item` is of this kind. `here` says whether the open chapter uses or pins it.
    pub fn admits(self, item: &Item, here: bool) -> bool {
        match self {
            Self::All => true,
            Self::ThisChapter => here,
            Self::Research => item.kind == Kind::Research,
            Self::Notes => item.kind == Kind::Notes,
            Self::Probes => item.kind == Kind::Probes,
            Self::Images => item.form == Form::Image,
            Self::Tables => item.form == Form::Table,
            Self::Sources => item.kind == Kind::Sources,
        }
    }

    /// A separator stands before these, as on the board: after This chapter, and after the folders.
    fn starts_group(self) -> bool {
        matches!(self, Self::Research | Self::Images)
    }
}

pub struct LibraryCard {
    material: Entity<Material>,
    editor: Entity<EditorView>,
    open: bool,
    input: Entity<TextInput>,
    filter: Filter,
    selected: usize,
    before: Option<FocusHandle>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl LibraryCard {
    pub fn new(material: Entity<Material>, editor: Entity<EditorView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextInput::new(window, cx)
                .bare()
                .keep_focus_on_enter()
                .arrows_to_parent()
                .with_label(Key::LibrarySearch)
                .with_placeholder(Key::LibrarySearch);
            input.pass_command_enter();
            input
        });
        let subscriptions = vec![
            cx.subscribe(&input, |card, _, _: &Edited, cx| {
                card.selected = 0;
                cx.notify();
            }),
            cx.subscribe_in(&input, window, |card, _, _: &Submitted, window, cx| card.open_in_rail(window, cx)),
            cx.subscribe_in(&input, window, |card, _, _: &Cancelled, window, cx| card.close(window, cx)),
            cx.subscribe(&material, |_, _, _: &MaterialChanged, cx| cx.notify()),
        ];
        Self {
            material,
            editor,
            open: false,
            input,
            filter: Filter::All,
            selected: 0,
            before: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(window, cx);
            return;
        }
        self.open = true;
        self.before = window.focused(cx);
        self.selected = 0;
        self.filter = Filter::All;
        self.input.update(cx, |input, cx| input.set_value(String::new(), true, cx));
        let handle = self.input.focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        let text = self.editor.focus_handle(cx);
        let back = self.before.take().unwrap_or(text);
        window.focus(&back, cx);
        cx.notify();
    }

    /// Whether the open chapter uses or pins an item.
    fn here(&self, id: &str, cx: &App) -> bool {
        let material = self.material.read(cx);
        material.uses_of(id).is_some() || material.pinned_here(id, cx)
    }

    /// The items the card lists: matching the query and the filter.
    fn results(&self, cx: &App) -> Vec<Item> {
        let material = self.material.read(cx);
        let query = self.input.read(cx).text().trim().to_owned();
        super::model::matching(material.items(), &query)
            .into_iter()
            .filter(|item| self.filter.admits(item, self.here(&item.id, cx)))
            .cloned()
            .collect()
    }

    fn count(&self, filter: Filter, cx: &App) -> usize {
        let material = self.material.read(cx);
        (material.items().iter())
            .filter(|item| filter.admits(item, self.here(&item.id, cx)))
            .count()
    }

    fn chosen(&self, cx: &App) -> Option<Item> {
        self.results(cx).into_iter().nth(self.selected)
    }

    fn step(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.results(cx).len();
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

    fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.selected = 0;
        cx.notify();
    }

    fn filter_at(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(filter) = Filter::ALL.get(at) {
            self.set_filter(*filter, cx);
        }
    }

    fn pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.chosen(cx) else {
            return;
        };
        self.close(window, cx);
        self.material.update(cx, |material, cx| material.pin(&item.id, None, cx));
    }

    fn insert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.chosen(cx) else {
            return;
        };
        self.close(window, cx);
        self.material
            .update(cx, |material, cx| material.insert(&item.id, None, window, cx));
    }

    fn open_in_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.chosen(cx) else {
            return;
        };
        self.close(window, cx);
        self.material.update(cx, |material, cx| material.open(&item.id, window, cx));
    }

    fn filters(&self, tokens: &Tokens, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut chips: Vec<AnyElement> = Vec::new();
        for filter in Filter::ALL {
            if filter.starts_group() {
                chips.push(div().w(tokens::HAIRLINE).h(px(14.)).mx(tokens::SPACE_1).bg(tokens.divider).into_any_element());
            }
            let on = filter == self.filter;
            let count = (filter != Filter::All).then(|| self.count(filter, cx));
            chips.push(
                div()
                    .id(SharedString::from(format!("library-filter-{filter:?}")))
                    .role(Role::Tab)
                    .aria_label(filter.label())
                    .aria_selected(on)
                    .flex_none()
                    .h(tokens::SMALL_BUTTON_HEIGHT)
                    .px(tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_1)
                    .rounded(tokens::CHIP_RADIUS)
                    .cursor_pointer()
                    .text_color(if on { tokens.text } else { tokens.text_quiet })
                    .when(on, |chip| chip.bg(tokens.raised))
                    .hover(|chip| chip.bg(tokens.raised))
                    .child(filter.label())
                    .when_some(count, |chip, count| {
                        chip.child(
                            div()
                                .text_size(tokens::TEXT_SMALL)
                                .text_color(tokens.text_faint)
                                .child(count.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |card, _, _, cx| card.set_filter(filter, cx)))
                    .into_any_element(),
            );
        }
        div()
            .id("library-filters")
            .flex_none()
            .px(tokens::SPACE_2)
            .py(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_1)
            .overflow_x_scroll()
            .border_b_1()
            .border_color(tokens.divider)
            .children(chips)
    }

    fn end_of(&self, item: &Item, cx: &App) -> String {
        let material = self.material.read(cx);
        match material.uses_of(&item.id).map(|uses| &uses.shown) {
            Some(Where::Figure(section) | Where::Section(section)) => section.clone(),
            Some(Where::Cited(count)) => crate::t!(Key::CitedCount, count = count),
            None => kind_label(item.kind).to_owned(),
        }
    }

    fn preview(&self, item: Option<&Item>, tokens: &Tokens, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = item else {
            return div().flex_1().into_any_element();
        };
        let material = self.material.read(cx);
        let picture = (item.form == Form::Image)
            .then(|| material.base().zip(item.path()).map(|(base, path)| base.join(path)))
            .flatten()
            .map(|file| {
                img(file)
                    .w_full()
                    .h(tokens::LIBRARY_IMAGE_HEIGHT)
                    .object_fit(ObjectFit::Contain)
                    .rounded(tokens::BOX_RADIUS)
            });
        let excerpt = (item.form == Form::Text).then(|| item.paragraphs().join(" ")).filter(|text| !text.is_empty());
        let body = self.editor.read(cx).body_font().family;
        let pins = material.pinned_count(&item.id);
        let path = item.path().unwrap_or(&item.id).to_owned();
        let pinned = (pins > 0).then(|| format!("{} {pins}", crate::t!(Key::Pinned)));
        div()
            .flex_1()
            .min_w_0()
            .p(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::HAIR_GAP)
            .children(picture)
            .child(div().mt(tokens::SPACE_2).font_weight(FontWeight::SEMIBOLD).truncate().child(item.title.clone()))
            .child(div().text_size(tokens::TEXT_SMALL).text_color(tokens.text_faint).truncate().child(path))
            .children(pinned.map(|pinned| div().text_size(tokens::TEXT_SMALL).text_color(tokens.text_faint).child(pinned)))
            .children(excerpt.map(|excerpt| {
                div()
                    .mt(tokens::SPACE_1)
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .font_family(body)
                    .text_size(tokens::PIN_EXCERPT_TEXT)
                    .line_height(tokens::PIN_EXCERPT_LINE)
                    .text_color(tokens.text_quiet)
                    .child(excerpt)
            }))
            .child(
                div()
                    .mt_auto()
                    .pt(tokens::SPACE_2)
                    .flex()
                    .flex_wrap()
                    .gap(tokens::SPACE_1)
                    .child(
                        small_button("library-pin", crate::t!(Key::PinHere).to_owned(), tokens)
                            .text_color(tokens.accent)
                            .on_click(cx.listener(|card, _, window, cx| card.pin(window, cx))),
                    )
                    .child(
                        small_button("library-insert", insert_label(item).to_owned(), tokens)
                            .on_click(cx.listener(|card, _, window, cx| card.insert(window, cx))),
                    )
                    .child(
                        small_button("library-open", crate::t!(Key::Open).to_owned(), tokens)
                            .on_click(cx.listener(|card, _, window, cx| card.open_in_rail(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Focusable for LibraryCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LibraryCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let tokens = Tokens::get(cx).clone();
        let results = self.results(cx);
        let all = self.material.read(cx).items().len();
        let selected = self.selected.min(results.len().saturating_sub(1));
        let rows: Vec<AnyElement> = (results.iter().enumerate())
            .map(|(at, item)| {
                let chosen = at == selected;
                div()
                    .id(SharedString::from(format!("library-row-{}", item.id)))
                    .role(Role::ListItem)
                    .aria_selected(chosen)
                    .aria_label(item.name().to_owned())
                    .flex_none()
                    .h(tokens::MATERIAL_ROW_HEIGHT)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .cursor_pointer()
                    .when(chosen, |row| row.bg(tokens.raised))
                    .hover(|row| row.bg(tokens.raised))
                    .child(svg().flex_none().path(icon(item)).size(tokens::ICON_SIZE).text_color(tokens.text_faint))
                    .child(div().flex_1().min_w_0().truncate().child(if item.form == Form::Source {
                        item.title.clone()
                    } else {
                        item.name().to_owned()
                    }))
                    .child(
                        div()
                            .flex_none()
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_faint)
                            .child(self.end_of(item, cx)),
                    )
                    .on_click(cx.listener(move |card, _, _, cx| {
                        card.selected = at;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let window_size = window.viewport_size();
        let width = tokens::LIBRARY_WIDTH.min(window_size.width - tokens::SPACE_4 * 2.);
        let left = TextColumn::centred(cx, width).min(window_size.width - width - tokens::SPACE_4);
        let height = tokens::LIBRARY_PANES_HEIGHT + tokens::TOOLBAR_HEIGHT * 3.;
        let below = self.editor.read(cx).caret_block_bottom().map(|bottom| bottom + tokens::SPACE_2);
        let top = match below {
            Some(top) if top + height <= window_size.height => top,
            _ => (window_size.height - height - tokens::FOOTER_HEIGHT).max(tokens::TITLEBAR_HEIGHT),
        };
        let count = crate::t!(Key::Count, all = all, shown = results.len());
        let chosen = results.get(selected).cloned();
        div()
            .id("library")
            .occlude()
            .absolute()
            .top(top)
            .left(left)
            .w(width)
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(tokens.panel)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .shadow(tokens::card_shadow())
            .font_family(tokens::FONT_UI)
            .text_size(tokens::TEXT_BODY)
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .on_action(cx.listener(|card, _: &input::Up, _, cx| card.step(false, cx)))
            .on_action(cx.listener(|card, _: &input::Down, _, cx| card.step(true, cx)))
            .on_action(cx.listener(|card, _: &input::Commit, window, cx| card.pin(window, cx)))
            .on_action(cx.listener(|card, _: &InsertChosen, window, cx| card.insert(window, cx)))
            .on_action(cx.listener(|card, _: &Filter1, _, cx| card.filter_at(0, cx)))
            .on_action(cx.listener(|card, _: &Filter2, _, cx| card.filter_at(1, cx)))
            .on_action(cx.listener(|card, _: &Filter3, _, cx| card.filter_at(2, cx)))
            .on_action(cx.listener(|card, _: &Filter4, _, cx| card.filter_at(3, cx)))
            .on_action(cx.listener(|card, _: &Filter5, _, cx| card.filter_at(4, cx)))
            .on_action(cx.listener(|card, _: &Filter6, _, cx| card.filter_at(5, cx)))
            .on_action(cx.listener(|card, _: &Filter7, _, cx| card.filter_at(6, cx)))
            .on_action(cx.listener(|card, _: &Filter8, _, cx| card.filter_at(7, cx)))
            .on_mouse_down_out(cx.listener(|card, _: &MouseDownEvent, window, cx| card.close(window, cx)))
            .child(
                div()
                    .flex_none()
                    .h(tokens::TOOLBAR_HEIGHT + tokens::SPACE_2)
                    .px(tokens::SPACE_3)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .child(svg().flex_none().path("icons/search.svg").size(tokens::ICON_SIZE).text_color(tokens.text_faint))
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(div().flex_none().text_size(tokens::TEXT_SMALL).text_color(tokens.text_faint).child(count)),
            )
            .child(self.filters(&tokens, cx))
            .child(
                div()
                    .flex_none()
                    .h(tokens::LIBRARY_PANES_HEIGHT)
                    .flex()
                    .child(
                        div()
                            .id("library-results")
                            .flex_none()
                            .w((width * 0.55).min(tokens::LIBRARY_RESULTS_WIDTH))
                            .py(tokens::SPACE_1)
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .border_r_1()
                            .border_color(tokens.divider)
                            .flex()
                            .flex_col()
                            .children(rows)
                            .when(results.is_empty(), |list| {
                                list.child(
                                    div()
                                        .p(tokens::SPACE_3)
                                        .text_color(tokens.text_faint)
                                        .child(crate::t!(Key::NoMatch)),
                                )
                            }),
                    )
                    .child(self.preview(chosen.as_ref(), &tokens, cx)),
            )
            .child(
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
                    .child(crate::t!(Key::KeyPin))
                    .child(crate::t!(Key::KeyInsert))
                    .child(crate::t!(Key::KeyOpenInRail)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_narrow_by_folder_by_form_and_by_the_chapter() {
        let item = |id: &str, kind, form| Item {
            id: id.to_owned(),
            kind,
            form,
            title: id.to_owned(),
            date: None,
            key: None,
            text: None,
        };
        let scan = item("research/a.png", Kind::Research, Form::Image);
        let probe = item("notes/probes/b.md", Kind::Probes, Form::Text);
        assert!(Filter::Images.admits(&scan, false));
        assert!(Filter::Research.admits(&scan, false));
        assert!(!Filter::Notes.admits(&probe, false));
        assert!(Filter::Probes.admits(&probe, false));
        assert!(Filter::ThisChapter.admits(&probe, true));
        assert!(!Filter::ThisChapter.admits(&scan, false));
    }
}
