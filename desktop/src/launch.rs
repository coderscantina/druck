//! The window with no document: open one, or start one from a starter. The starters are a gallery: each shows a page of
//! its sample, laid out by the engine and painted as Publish paints pages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, EventEmitter, FocusHandle, FontWeight, KeyDownEvent, Pixels, RenderImage, SharedString, Task, Window, div,
    img, px, svg,
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
    /// Each starter's page by its slug, once painted, in the language of `painted`.
    pages: HashMap<&'static str, Arc<RenderImage>>,
    painted: Language,
    /// Pages of another language, to free at the next frame.
    stale: Vec<Arc<RenderImage>>,
    _painting: Task<()>,
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
            pages: HashMap::new(),
            painted: language(),
            stale: Vec::new(),
            _painting: Task::ready(()),
        };
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
        let starters: Vec<_> = STARTERS
            .iter()
            .map(|starter| self.starter_card(&tokens, starter, starter_width, cx))
            .collect();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(|_: &crate::workspace::FocusNext, window, cx| crate::a11y::tab(true, window, cx))
            .on_action(|_: &crate::workspace::FocusPrevious, window, cx| crate::a11y::tab(false, window, cx))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(LaunchEvent::Dismiss);
                }
            }))
            .bg(tokens.canvas)
            .child(
                div()
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
                    .when_some(self.offer.clone(), |card, (dir, offer)| {
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
                    .child(div().px(tokens::SPACE_2).pb(tokens::SPACE_2).flex().children(starters)),
            )
    }
}
