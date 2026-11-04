//! The footer strip's word count: the open chapter and the whole book.

use gpui::prelude::*;
use gpui::{Context, Window, div};

use super::text::Key;
use crate::tokens::{self, Tokens};

#[derive(Default)]
pub struct Footer {
    chapter: Option<usize>,
    book: Option<usize>,
}

impl Footer {
    pub fn set(&mut self, chapter: Option<usize>, book: Option<usize>, cx: &mut Context<Self>) {
        if (self.chapter, self.book) != (chapter, book) {
            (self.chapter, self.book) = (chapter, book);
            cx.notify();
        }
    }
}

/// A number with thousands separated by a thin space.
fn grouped(number: usize) -> String {
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let text = match (self.chapter, self.book) {
            (Some(chapter), Some(book)) => {
                crate::t!(Key::WordsFooter, chapter = grouped(chapter), book = grouped(book))
            }
            (Some(chapter), None) => crate::t!(Key::WordsChapterOnly, chapter = grouped(chapter)),
            _ => String::new(),
        };
        // Takes only what it needs: a conflict in the same strip needs the room for its sentence.
        div()
            .flex_none()
            .ml_auto()
            .px(tokens::SPACE_3)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
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
