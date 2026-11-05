//! The Images tab: every image of the project with how often the book uses it, and the files the text names that are not
//! there.

use gpui::prelude::*;
use gpui::{
    Context, Entity, FontWeight, ObjectFit, ScrollHandle, SharedString, Subscription, Task, Window, div, img, svg,
};

use crate::a11y::Control as _;
use crate::editor::rich::images::{self, Listed};
use crate::editor::rich::view::RichChanged;
use crate::editor::rich::{Key, view::shapes::size_label};
use crate::editor::{EditorView, SessionChanged};
use crate::session::{ChapterId, Kind};
use crate::tokens::{self, Tokens};

/// One row, with the first place the book uses it.
#[derive(Clone, PartialEq, Eq)]
struct Row {
    listed: Listed,
    first_use: Option<(ChapterId, usize)>,
    size: Option<String>,
    problem: Option<String>,
}

pub struct ImagesPanel {
    editor: Entity<EditorView>,
    rows: Vec<Row>,
    /// What the text used when last listed, so a pause in typing does not read the folders again.
    uses: Vec<String>,
    /// Decodes the listed files off the main thread, as the engine would.
    inspecting: Task<()>,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ImagesPanel {
    pub fn new(editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&editor, |panel, _, _: &RichChanged, cx| panel.sync(false, cx)),
            cx.subscribe(&editor, |panel, _, _: &SessionChanged, cx| panel.sync(true, cx)),
        ];
        let mut panel = Self {
            editor,
            rows: Vec::new(),
            uses: Vec::new(),
            inspecting: Task::ready(()),
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        panel.sync(true, cx);
        panel
    }

    /// Lists the images again, when the text uses others or `folders` asks for a fresh look. Draws only when the list
    /// changed, and each file's size and problems follow from another thread.
    fn sync(&mut self, folders: bool, cx: &mut Context<Self>) {
        let (uses, base, is_project) = {
            let editor = self.editor.read(cx);
            let session = editor.session();
            (
                editor.image_uses(),
                session.base_dir(),
                matches!(session.kind(), Kind::Project { .. }),
            )
        };
        let paths: Vec<String> = uses.iter().map(|(_, path, _)| path.clone()).collect();
        if !folders && paths == self.uses {
            return;
        }
        let listed = images::list(&base, is_project, paths.iter().cloned());
        self.uses = paths;
        if listed.len() == self.rows.len() && listed.iter().zip(&self.rows).all(|(new, row)| *new == row.listed) {
            return;
        }
        self.rows = listed
            .into_iter()
            .map(|listed| Row {
                first_use: uses
                    .iter()
                    .find(|(_, path, _)| *path == listed.path)
                    .map(|(chapter, _, offset)| (*chapter, *offset)),
                listed,
                size: None,
                problem: None,
            })
            .collect();
        cx.notify();
        let files: Vec<_> = self
            .rows
            .iter()
            .filter(|row| !row.listed.missing)
            .map(|row| row.listed.file.clone())
            .collect();
        let found = cx.background_spawn(async move {
            files
                .into_iter()
                .map(|file| {
                    let inspected = images::inspect(&file);
                    (file, inspected)
                })
                .collect::<Vec<_>>()
        });
        self.inspecting = cx.spawn(async move |panel, cx| {
            let found = found.await;
            panel
                .update(cx, |panel, cx| {
                    for (file, inspected) in found {
                        if let Some(row) = panel.rows.iter_mut().find(|row| row.listed.file == file) {
                            match inspected {
                                Ok(size) => row.size = Some(size_label(size).to_string()),
                                Err(why) => row.problem = Some(why),
                            }
                        }
                    }
                    cx.notify();
                })
                .ok();
        });
    }
}

impl Render for ImagesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let header = crate::inspector::widgets::header(&tokens, crate::t!(Key::ImagesTitle));
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let name: SharedString = row.listed.path.clone().into();
                let uses: SharedString = if row.listed.missing {
                    crate::t!(Key::ImageNoFile).into()
                } else {
                    match row.listed.uses {
                        0 => crate::t!(Key::ImageUnused).into(),
                        1 => crate::t!(Key::ImageUsedOnce).into(),
                        count => crate::t!(Key::ImageUses, count = count).into(),
                    }
                };
                let line: SharedString = match (&row.problem, &row.size) {
                    (Some(problem), _) => problem.clone().into(),
                    (None, Some(size)) => format!("{size}  ·  {uses}").into(),
                    (None, None) => uses,
                };
                let wrong = row.listed.missing || row.problem.is_some();
                let place = row.first_use;
                let editor = self.editor.clone();
                div()
                    .id(("image-row", index))
                    .button(&tokens, row.listed.path.clone())
                    .flex_none()
                    .min_h(tokens::IMAGE_THUMB + tokens::SPACE_2 * 2.)
                    .px(tokens::SPACE_3)
                    .py(tokens::SPACE_2)
                    .flex()
                    .items_center()
                    .gap(tokens::SPACE_2)
                    .border_b_1()
                    .border_color(tokens.divider)
                    .when(place.is_some(), |row| {
                        row.cursor_pointer().hover(|row| row.bg(tokens.raised))
                    })
                    .on_click(move |_, window, cx| {
                        if let Some((chapter, offset)) = place {
                            editor.update(cx, |editor, cx| editor.show_place(chapter, offset, window, cx));
                        }
                    })
                    .child(
                        div()
                            .flex_none()
                            .size(tokens::IMAGE_THUMB)
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(tokens::CHIP_RADIUS)
                            .overflow_hidden()
                            .bg(tokens.raised)
                            .when(!row.listed.missing, |thumb| {
                                thumb.child(
                                    img(row.listed.file.clone())
                                        .size(tokens::IMAGE_THUMB)
                                        .object_fit(ObjectFit::Contain),
                                )
                            })
                            .when(row.listed.missing, |thumb| {
                                thumb.child(
                                    svg()
                                        .path("icons/images.svg")
                                        .size(tokens::ICON_SIZE)
                                        .text_color(tokens.accent),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().text_color(tokens.text).child(name))
                            .child(
                                div()
                                    .text_size(tokens::TEXT_SMALL)
                                    .text_color(if wrong { tokens.accent } else { tokens.text_faint })
                                    .child(line),
                            ),
                    )
            })
            .collect();
        let empty = self.rows.is_empty().then(|| {
            div()
                .p(tokens::SPACE_3)
                .text_size(tokens::TEXT_SMALL)
                .text_color(tokens.text_faint)
                .child(crate::t!(Key::ImagesEmpty))
        });
        let editor = self.editor.clone();
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .text_size(tokens::TEXT_BODY)
            .font_weight(FontWeight::NORMAL)
            .child(header)
            .child(
                div()
                    .id("images-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .children(empty),
            )
            .child(
                div()
                    .flex_none()
                    .p(tokens::SPACE_3)
                    .border_t_1()
                    .border_color(tokens.divider)
                    .child(
                        crate::inspector::widgets::button(&tokens, "images-insert", crate::t!(Key::ImagesInsert))
                            .on_click(move |_, window, cx| {
                                editor.update(cx, |editor, cx| editor.choose_image(None, window, cx));
                            }),
                    ),
            )
    }
}
