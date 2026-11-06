//! A probe's choice of type: the book's theme, or one other theme from the project's or the shared ones, setting the
//! passage in that theme's body face. Both are scaled alike, so the book's body stands at the panel's reading size and
//! the other theme's in proportion to it. Each theme's type is read once, off the main thread, without a layout.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{AnyElement, Context, Entity, Pixels, SharedString, div};

use super::Key;
use super::model::{Item, Kind};
use super::panel::MaterialPanel;
use crate::editor::EditorView;
use crate::preview::BookType;
use crate::preview::job::Typesetter;
use crate::session::Joined;
use crate::tokens::{self, Tokens};

/// A theme a probe can be set in.
#[derive(Clone, PartialEq)]
struct Theme {
    name: String,
    path: PathBuf,
}

#[derive(Default)]
pub struct ProbeChoice {
    /// The book's theme, and the others, as of the last probe opened.
    book: Option<Theme>,
    others: Vec<Theme>,
    /// Which of the others is chosen, `None` for the book's.
    chosen: Option<usize>,
    /// Each theme's type once read, `None` if it could not be.
    types: HashMap<PathBuf, Option<BookType>>,
    /// The font files the toolkit has been given, by their hash.
    fonts: HashSet<u64>,
    reading: Vec<gpui::Task<()>>,
    /// The probe last opened, so drawing it again reads nothing.
    opened: Option<String>,
    /// The book's folder, where a theme's relative paths start.
    base: PathBuf,
    /// A theme asked for by name before the themes were listed. For screenshots.
    #[cfg(any(debug_assertions, feature = "measure"))]
    wanted: Option<String>,
}

impl ProbeChoice {
    /// A probe opened: finds the themes, and reads the book's type if that is still to do.
    pub fn item_opened(&mut self, id: &str, editor: &Entity<EditorView>, cx: &mut Context<MaterialPanel>) {
        if !id.starts_with(super::model::PROBES) || self.opened.as_deref() == Some(id) {
            return;
        }
        self.opened = Some(id.to_owned());
        let session = editor.read(cx).session();
        self.base = session.base_dir();
        let dirs = session.theme_dirs();
        let book = session.theme_file();
        let entries = crate::theme_file::library::list(dirs.project.as_deref(), dirs.shared.as_deref());
        self.book = book.clone().map(|path| Theme {
            name: path
                .parent()
                .and_then(|folder| folder.file_name())
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            path,
        });
        let others: Vec<Theme> = entries
            .into_iter()
            .filter(|entry| Some(&entry.path) != book.as_ref())
            .map(|entry| Theme {
                name: entry.name,
                path: entry.path,
            })
            .collect();
        if others != self.others {
            self.chosen = None;
            self.others = others;
        }
        if let Some(book) = self.book.clone() {
            self.read(book.path, cx);
        }
        #[cfg(any(debug_assertions, feature = "measure"))]
        if let Some(name) = self.wanted.take() {
            self.choose_named(&name, cx);
        }
    }

    /// Reads a theme's type on another thread, and gives its fonts to the toolkit once read.
    fn read(&mut self, path: PathBuf, cx: &mut Context<MaterialPanel>) {
        if self.types.contains_key(&path) {
            return;
        }
        self.types.insert(path.clone(), None);
        let kind = crate::session::Kind::Single {
            path: self.base.join("probe.md"),
        };
        let theme = path.clone();
        let read = cx.background_spawn(async move {
            Typesetter::new(kind, Some(theme)).read_type(Joined::single("Probe\n".to_owned(), 0))
        });
        self.reading.push(cx.spawn(async move |panel, cx| {
            let book_type = read.await;
            panel
                .update(cx, |panel, cx| {
                    if let Some(book_type) = &book_type {
                        panel.probe.give_fonts(book_type, cx);
                    }
                    panel.probe.types.insert(path, book_type);
                    cx.notify();
                })
                .ok();
        }));
    }

    fn give_fonts(&mut self, book_type: &BookType, cx: &mut Context<MaterialPanel>) {
        let files: Vec<std::borrow::Cow<'static, [u8]>> = (book_type.files.iter())
            .filter(|file| self.fonts.insert(file.hash()))
            .map(|file| std::borrow::Cow::Owned(file.to_vec()))
            .collect();
        if !files.is_empty() {
            // A font the toolkit refuses leaves the probe in a face it has, which is all that is lost.
            cx.text_system().add_fonts(files).ok();
        }
    }

    fn chosen_theme(&self) -> Option<&Theme> {
        match self.chosen {
            Some(at) => self.others.get(at),
            None => self.book.as_ref(),
        }
    }

    /// Book theme, then the other theme's name. Choosing the other again goes on to the next one.
    pub fn choice(&self, tokens: &Tokens, cx: &mut Context<MaterialPanel>) -> AnyElement {
        if self.others.is_empty() {
            return div().into_any_element();
        }
        let other = self.others[self.chosen.unwrap_or(0)].name.clone();
        let segment = |id: &'static str, label: String, on: bool| {
            div()
                .id(id)
                .role(gpui::Role::RadioButton)
                .aria_label(label.clone())
                .aria_selected(on)
                .px(tokens::SPACE_2)
                .rounded(tokens::BOX_RADIUS)
                .cursor_pointer()
                .whitespace_nowrap()
                .text_color(if on { tokens.text } else { tokens.text_quiet })
                .when(on, |segment| segment.bg(tokens.raised))
                .child(label)
        };
        let book_on = self.chosen.is_none();
        div()
            .flex_none()
            .flex()
            .gap(tokens::HAIR_GAP)
            .p(tokens::HAIR_GAP)
            .rounded(tokens::RADIUS)
            .bg(tokens.field)
            .text_size(tokens::TEXT_SMALL)
            .child(
                segment("probe-book", crate::t!(Key::BookTheme).to_owned(), book_on).on_click(cx.listener(
                    |panel, _, _, cx| {
                        panel.probe.chosen = None;
                        cx.notify();
                    },
                )),
            )
            .child(segment("probe-other", other, !book_on).on_click(cx.listener(|panel, _, _, cx| {
                panel.probe.choose_next(cx);
            })))
            .into_any_element()
    }

    fn choose_next(&mut self, cx: &mut Context<MaterialPanel>) {
        if self.others.is_empty() {
            return;
        }
        self.chosen = Some(self.chosen.map_or(0, |at| (at + 1) % self.others.len()));
        if let Some(theme) = self.chosen_theme().cloned() {
            self.read(theme.path, cx);
        }
        cx.notify();
    }

    /// The family, size, and line height a probe is set in, once its theme's type has been read.
    pub fn type_for(&self, item: &Item) -> Option<(SharedString, Pixels, Pixels)> {
        if item.kind != Kind::Probes {
            return None;
        }
        let book = self.types.get(&self.book.as_ref()?.path)?.as_ref()?;
        let theme = self.types.get(&self.chosen_theme()?.path)?.as_ref()?;
        let scale = f32::from(tokens::MATERIAL_READING_TEXT) / book.body.size.max(1.);
        let size = theme.body.size * scale;
        Some((
            theme.body.family.clone().into(),
            gpui::px(size),
            gpui::px(size * theme.body.line_height),
        ))
    }

    /// The quiet line under a probe set in another theme.
    pub fn note(&self, item: &Item) -> Option<String> {
        let theme = self.chosen.and_then(|at| self.others.get(at))?;
        (item.kind == Kind::Probes).then(|| crate::t!(Key::SetIn, theme = theme.name))
    }

    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn choose_named(&mut self, name: &str, cx: &mut Context<MaterialPanel>) {
        match self.others.iter().position(|theme| theme.name == name) {
            Some(at) => {
                self.chosen = Some(at);
                let path = self.others[at].path.clone();
                self.read(path, cx);
            }
            None => self.wanted = Some(name.to_owned()),
        }
    }
}
