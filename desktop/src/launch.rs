//! The window with no document: open one, or start one from a starter. The starters are a gallery: each shows a page of
//! its sample, laid out by the engine and painted as Publish paints pages. Until it has an answer, the card also asks
//! once whether to turn the writing partner on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, KeyDownEvent, Pixels, RenderImage, ScrollHandle, SharedString,
    Task, Window, div, img, point, px, svg,
};

use crate::a11y::Control as _;
use crate::session::adopt::{self, Offer};
use crate::session::{STARTERS, Starter};
use crate::shell::strings::{Language, language};
use crate::tokens::{self, Tokens};

#[path = "launch_strings.rs"]
mod launch_strings;
use launch_strings::Key;

pub enum LaunchEvent {
    Open,
    New(&'static Starter),
    MakeProject(PathBuf, Offer),
    /// Escape: back to the book, when the card was asked for over one.
    Dismiss,
}

impl EventEmitter<LaunchEvent> for LaunchView {}

pub struct LaunchView {
    /// Why the last attempt to open something failed.
    error: Option<SharedString>,
    /// A folder of Markdown that failed for having no manifest, and what making it a project would do.
    offer: Option<(PathBuf, Offer)>,
    /// With nothing focused Tab has nowhere to start from, so the card holds the keys.
    focus: FocusHandle,
    /// The offer's row, which gets the keys while it shows.
    offer_focus: FocusHandle,
    /// Each starter's card, in the order of [`STARTERS`], so the arrow keys can move between them.
    starter_focus: Vec<FocusHandle>,
    /// The window's scroll, for a card taller than the window.
    scroll: ScrollHandle,
    /// Each starter's page by its slug, once painted, in the language of `painted`.
    pages: HashMap<&'static str, Arc<RenderImage>>,
    painted: Language,
    /// Pages of another language, to free at the next frame.
    stale: Vec<Arc<RenderImage>>,
    _painting: Task<()>,
    _settings: gpui::Subscription,
}

/// A sheet's box is this many times as high as wide, the tallest starter page, so the cards never move as pages come.
const SHEET_BOX_RATIO: f32 = 1.55;
/// The pixels a page is painted at: twice the widest sheet, for a dense display.
const PAGE_PIXELS: u32 = 336;

impl LaunchView {
    /// `error` says why what was asked for failed to open, and what that was.
    pub fn new(error: Option<(String, PathBuf)>, cx: &mut Context<Self>) -> Self {
        let (error, asked) = error.unzip();
        let mut view = Self {
            error: None,
            offer: None,
            focus: cx.focus_handle(),
            offer_focus: cx.focus_handle().tab_stop(true),
            starter_focus: STARTERS.iter().map(|_| cx.focus_handle().tab_stop(true)).collect(),
            scroll: ScrollHandle::new(),
            pages: HashMap::new(),
            painted: language(),
            stale: Vec::new(),
            _painting: Task::ready(()),
            // The offer goes once it is answered, here or by the switch in Settings.
            _settings: cx.observe_global::<crate::settings::Store>(|_, cx| cx.notify()),
        };
        // `DRUCK_OFFER=yes` or `no` answers the offer as its buttons do, for scripted runs.
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(answer) = crate::gate::var("DRUCK_OFFER") {
            crate::settings::change(cx, |settings| settings.answer_agent_offer(answer == "yes"));
        }
        view.take_error(error, asked.as_deref());
        view.paint_pages(cx);
        view
    }

    /// Lays out and paints each starter's sample off the main thread, one after another, in the app's language.
    fn paint_pages(&mut self, cx: &mut Context<Self>) {
        let language = language();
        self.painted = language;
        self.stale.extend(self.pages.drain().map(|(_, page)| page));
        self._painting = cx.spawn(async move |view, cx| {
            for starter in STARTERS {
                let painted = cx
                    .background_spawn(async move { crate::preview::gallery::page(starter, language, PAGE_PIXELS) })
                    .await;
                let shown = match painted {
                    Ok(pixmap) => view.update(cx, |view, cx| {
                        let (page, _) = crate::preview::render_image(pixmap);
                        view.pages.insert(starter.slug, page);
                        cx.notify();
                    }),
                    Err(problem) => {
                        eprintln!("{problem}");
                        Ok(())
                    }
                };
                if shown.is_err() {
                    return;
                }
            }
        });
    }

    /// A starter's card: its page on a sheet, then its name and what it holds. `width` is the card's.
    fn starter_card(
        &self,
        tokens: &Tokens,
        starter: &'static Starter,
        focus: &FocusHandle,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let name = crate::t!(starter.name);
        let sheet_width = width - tokens::SPACE_2 * 2.;
        let sheet = self.pages.get(starter.slug).map(|page| {
            let size = page.size(0);
            let height = sheet_width * (size.height.0 as f32 / size.width.0 as f32);
            div()
                .border_1()
                .border_color(tokens.divider)
                .child(img(page.clone()).w(sheet_width - px(2.)).h(height - px(2.)))
        });
        div()
            .id(starter.slug)
            .button(tokens, name)
            .track_focus(focus)
            .w(width)
            .p(tokens::SPACE_2)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .rounded(tokens::RADIUS)
            .cursor_pointer()
            .hover(|card| card.bg(tokens.raised))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(LaunchEvent::New(starter))))
            .child(
                div()
                    .h(sheet_width * SHEET_BOX_RATIO)
                    .flex()
                    .items_end()
                    .children(sheet),
            )
            .child(
                div().child(name).child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(crate::t!(starter.description)),
                ),
            )
    }

    /// Left and Right move the keys between the starters, stopping at the first and the last.
    fn step_starters(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(at) = self.starter_focus.iter().position(|focus| focus.is_focused(window)) else {
            return false;
        };
        let next = if forward {
            (at + 1).min(self.starter_focus.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        window.focus(&self.starter_focus[next], cx);
        true
    }

    /// The first launch's question about the writing partner: what it does, what it needs, and the two answers.
    fn agent_offer(tokens: &Tokens) -> impl IntoElement + use<> {
        let answer = |id: &'static str, label: Key, yes: bool| {
            crate::editor::review::small_button(id, crate::t!(label).to_owned(), tokens)
                .on_click(move |_, _, cx| crate::settings::change(cx, |settings| settings.answer_agent_offer(yes)))
        };
        let quiet = |text: Key| {
            div()
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_quiet)
                .child(crate::t!(text))
        };
        div()
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_3)
            .flex()
            .flex_col()
            .gap(tokens::SPACE_2)
            .child(quiet(Key::AgentOffer).font_weight(FontWeight::SEMIBOLD))
            .child(div().child(crate::t!(Key::AgentOfferWhat)))
            .child(quiet(Key::AgentOfferNeeds))
            .child(
                div()
                    .flex()
                    .gap(tokens::SPACE_2)
                    .child(answer("agent-offer-yes", Key::AgentOfferYes, true).text_color(tokens.accent))
                    .child(answer("agent-offer-no", Key::AgentOfferNo, false)),
            )
    }

    /// Moves the keys and keeps their control in a window the card outgrows: the starters are its last row, so one of
    /// them shows the card's foot, and anything else its head.
    fn tab(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        crate::a11y::tab(forward, window, cx);
        if self.starter_focus.iter().any(|focus| focus.is_focused(window)) {
            self.scroll.scroll_to_bottom();
        } else {
            self.scroll.set_offset(point(px(0.), px(0.)));
        }
        cx.notify();
    }

    /// A folder that only lacks a manifest is no error: the offer's own line says what is missing.
    fn take_error(&mut self, error: Option<String>, asked: Option<&Path>) {
        self.offer = asked.and_then(offer_for);
        self.error = error.filter(|_| self.offer.is_none()).map(SharedString::from);
    }

    /// Gives the keys to the offer if there is one, or the card, as the window shows it.
    pub fn take_focus(view: &gpui::Entity<Self>, window: &mut Window, cx: &mut gpui::App) {
        let view = view.read(cx);
        let focus = if view.offer.is_some() {
            view.offer_focus.clone()
        } else {
            view.focus.clone()
        };
        window.focus(&focus, cx);
    }

    /// Says why `asked` did not open, or with `None`, that nothing is wrong.
    pub fn set_error(&mut self, error: Option<String>, asked: Option<&Path>, cx: &mut Context<Self>) {
        self.take_error(error, asked);
        cx.notify();
    }

    /// Says why the offer did not work. The offer stays, to try again.
    pub fn offer_failed(&mut self, problem: String, cx: &mut Context<Self>) {
        self.error = Some(problem.into());
        cx.notify();
    }

    fn row(
        tokens: &Tokens,
        id: &'static str,
        icon: &'static str,
        name: &'static str,
        description: impl Into<SharedString>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .button(tokens, name)
            .px(tokens::SPACE_3)
            .py(tokens::SPACE_2)
            .flex()
            .items_center()
            .gap(tokens::SPACE_3)
            .cursor_pointer()
            .hover(|row| row.bg(tokens.raised))
            .child(
                svg()
                    .flex_none()
                    .path(icon)
                    .size(tokens::ICON_SIZE)
                    .text_color(tokens.text_quiet),
            )
            .child(
                div().flex_1().min_w_0().child(name).child(
                    div()
                        .text_size(tokens::TEXT_SMALL)
                        .text_color(tokens.text_quiet)
                        .child(description.into()),
                ),
            )
    }
}

fn offer_for(path: &Path) -> Option<(PathBuf, Offer)> {
    adopt::offer(path).map(|offer| (path.to_owned(), offer))
}

impl Render for LaunchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        for page in self.stale.drain(..) {
            window.drop_image(page).ok();
        }
        if self.painted != language() {
            self.paint_pages(cx);
        }
        // The card keeps a gutter to the window's edge, so five starters fit the smallest window.
        let width = (window.viewport_size().width - tokens::SPACE_4 * 2.).min(tokens::LAUNCH_WIDTH);
        let starter_width = ((width - tokens::SPACE_2 * 2. - px(2.)) / STARTERS.len() as f32).floor();
        let offering = crate::settings::get(cx).offers_agent();
        let starters: Vec<_> = (STARTERS.iter().zip(&self.starter_focus))
            .map(|(starter, focus)| {
                self.starter_card(&tokens, starter, focus, starter_width, cx)
                    .into_any_element()
            })
            .collect();
        // The card stands in the middle while it fits, and the window scrolls when it does not.
        div()
            .id("launch")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|view, _: &crate::workspace::FocusNext, window, cx| view.tab(true, window, cx)))
            .on_action(cx.listener(|view, _: &crate::workspace::FocusPrevious, window, cx| view.tab(false, window, cx)))
            .on_key_down(cx.listener(
                |view, event: &KeyDownEvent, window, cx| match event.keystroke.key.as_str() {
                    "escape" => cx.emit(LaunchEvent::Dismiss),
                    "left" | "right" if view.step_starters(event.keystroke.key == "right", window, cx) => {
                        cx.stop_propagation();
                    }
                    _ => {}
                },
            ))
            .bg(tokens.canvas)
            .child(
                div()
                    .min_h_full()
                    .py(tokens::SPACE_4)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.card(&tokens, width, offering, starters, cx)),
            )
    }
}

impl LaunchView {
    fn card(
        &self,
        tokens: &Tokens,
        width: Pixels,
        offering: bool,
        starters: Vec<gpui::AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tokens = tokens.clone();
        div()
            .flex_none()
            .w(width)
            .flex()
            .flex_col()
            .bg(tokens.canvas)
            .border_1()
            .border_color(tokens.divider)
            .rounded(tokens::RADIUS)
            .child(
                div()
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .text_size(tokens::TEXT_SMALL)
                    .text_color(tokens.text_quiet)
                    .child("Druck"),
            )
            // The question comes before anything else, alone, so it cannot crowd the starters out of a small
            // window.
            .when(offering, |card| card.child(Self::agent_offer(&tokens)))
            .when(!offering, |card| {
                card.when_some(self.offer.clone(), |card, (dir, offer)| {
                    card.child(
                        div()
                            .px(tokens::SPACE_3)
                            .pt(tokens::SPACE_2)
                            .text_size(tokens::TEXT_SMALL)
                            .text_color(tokens.text_quiet)
                            .child(adopt::reason(&dir)),
                    )
                    .child(
                        Self::row(&tokens, "make-project", "icons/book.svg", adopt::label(), offer.hint())
                            .track_focus(&self.offer_focus)
                            .ring_while_focused(&tokens)
                            .border_b_1()
                            .border_color(tokens.divider)
                            .text_color(tokens.accent)
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(LaunchEvent::MakeProject(dir.clone(), offer.clone()));
                            })),
                    )
                })
                .when_some(self.error.clone(), |card, error| {
                    card.child(div().p(tokens::SPACE_3).text_color(tokens.accent).child(error))
                })
                .child(
                    Self::row(
                        &tokens,
                        "open",
                        "icons/file.svg",
                        crate::t!(Key::Open),
                        crate::t!(Key::OpenHint),
                    )
                    .border_b_1()
                    .border_color(tokens.divider)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(LaunchEvent::Open))),
                )
                .child(
                    div()
                        .px(tokens::SPACE_3)
                        .pt(tokens::SPACE_3)
                        .pb(tokens::SPACE_1)
                        .text_size(tokens::TEXT_SMALL)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tokens.text_quiet)
                        .child(crate::t!(Key::NewProject)),
                )
                .child(div().px(tokens::SPACE_2).pb(tokens::SPACE_2).flex().children(starters))
            })
    }
}
