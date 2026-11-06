//! The word count at the bottom right of the bare page: the open chapter of the whole book, faint, with a thin space
//! between thousands. It hides while typing. A change of zoom shows its value there for a moment.

use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, Entity, Subscription, Task, Window, div};

use super::text::Key;
use crate::editor::{EditorView, RoomChanged};
use crate::tokens::{self, Tokens};

/// How long after the last key the count comes back.
const TYPING_PAUSE: Duration = Duration::from_millis(1200);
/// How long a zoom value shows.
const ZOOM_TIME: Duration = Duration::from_millis(1200);

pub struct Footer {
    /// Says how much room the text leaves at its right.
    editor: Entity<EditorView>,
    chapter: Option<usize>,
    book: Option<usize>,
    /// Only Write has it.
    shown: bool,
    typing: bool,
    typing_pause: Task<()>,
    zoom: Option<u32>,
    zoom_pause: Task<()>,
    _room: Subscription,
}

impl Footer {
    pub fn new(editor: Entity<EditorView>, cx: &mut Context<Self>) -> Self {
        // Whether the count fits is known only once the text is laid out, after this has drawn.
        let room = cx.subscribe(&editor, |_, _, _: &RoomChanged, cx| cx.notify());
        Self {
            _room: room,
            editor,
            chapter: None,
            book: None,
            shown: true,
            typing: false,
            typing_pause: Task::ready(()),
            zoom: None,
            zoom_pause: Task::ready(()),
        }
    }

    pub fn set(&mut self, chapter: Option<usize>, book: Option<usize>, cx: &mut Context<Self>) {
        if (self.chapter, self.book) != (chapter, book) {
            (self.chapter, self.book) = (chapter, book);
            cx.notify();
        }
    }

    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.shown = shown;
        cx.notify();
    }

    /// A key was typed. Does nothing else on a key, once the count has gone.
    pub fn typed(&mut self, cx: &mut Context<Self>) {
        if !self.typing {
            self.typing = true;
            cx.notify();
        }
        self.typing_pause = cx.spawn(async move |footer, cx| {
            cx.background_executor().timer(TYPING_PAUSE).await;
            footer
                .update(cx, |footer, cx| {
                    footer.typing = false;
                    cx.notify();
                })
                .ok();
        });
    }

    /// Shows a zoom level for a moment, as a percentage.
    pub fn show_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = Some((zoom * 100.).round() as u32);
        cx.notify();
        self.zoom_pause = cx.spawn(async move |footer, cx| {
            cx.background_executor().timer(ZOOM_TIME).await;
            footer
                .update(cx, |footer, cx| {
                    footer.zoom = None;
                    cx.notify();
                })
                .ok();
        });
    }
}

/// Whether the count stands clear of the text, which it would otherwise be drawn over in a narrow window.
fn fits(count: &str, room: gpui::Pixels, window: &Window) -> bool {
    super::ui::small_text_width(count, window) + tokens::SPACE_3 * 2. <= room
}

/// A number with thousands separated by a thin space.
pub(crate) fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    out
}

impl Render for Footer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let count = match (self.chapter, self.book) {
            (Some(chapter), Some(book)) => {
                crate::t!(Key::WordsFooter, chapter = grouped(chapter), book = grouped(book))
            }
            (Some(chapter), None) => crate::t!(Key::WordsChapterOnly, chapter = grouped(chapter)),
            _ => String::new(),
        };
        let text = match self.zoom {
            Some(zoom) => format!("{zoom}\u{202f}%"),
            None if self.shown && !self.typing && fits(&count, self.editor.read(cx).room_beside_text(), window) => {
                count
            }
            None => String::new(),
        };
        // Takes only what it needs: a conflict in the same line needs the room for its sentence.
        div()
            .flex_none()
            .ml_auto()
            .px(tokens::SPACE_3)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .when(!text.is_empty(), |count| count.bg(tokens.canvas))
            .child(text)
    }
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn thousands_are_separated() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(12480), "12\u{202f}480");
        assert_eq!(grouped(1234567), "1\u{202f}234\u{202f}567");
    }
}
