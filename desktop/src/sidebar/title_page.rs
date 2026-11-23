//! The title page, edited where it stands in place of the text: the book's title, subtitle, author, and publisher,
//! centred in the book's type. Each line is a one-line field that writes the book's details, as the details panel does.
//!
//! It holds no document. The workspace gives it the details and the type, and hears [`TitlePageEvent`]s.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Font, FontFeatures, FontStyle, FontWeight, Pixels,
    SharedString, Subscription, TextRun, Window, canvas, div, font, px, svg,
};

use super::strings::Key;
use crate::editor::Column;
use crate::inspector::input::{Cancelled, Committed, Edited, Submitted, TextInput};
use crate::preview::BookType;
use crate::session::{Details, Field, Session};
use crate::tokens::{self, Tokens};
use crate::workspace::{FocusNext, FocusPrevious};

/// The `meta` key the publisher is written under.
const PUBLISHER: &str = "publisher";

/// A line of the title page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Title,
    Subtitle,
    Author,
    Publisher,
}

impl Line {
    const ALL: [Self; 4] = [Self::Title, Self::Subtitle, Self::Author, Self::Publisher];

    fn name(self) -> &'static str {
        crate::t!(match self {
            Self::Title => Key::TitleLine,
            Self::Subtitle => Key::SubtitleLine,
            Self::Author => Key::AuthorLine,
            Self::Publisher => Key::PublisherLine,
        })
    }

    /// What the book's details say for the line. `title` is the title as written, without the file name that stands in
    /// for a missing one.
    fn read(self, details: &Details, title: &str) -> String {
        match self {
            Self::Title => title.to_owned(),
            Self::Subtitle => details.subtitle.clone(),
            Self::Author => details.authors.clone(),
            Self::Publisher => (details.meta.iter())
                .find(|row| row.key == PUBLISHER)
                .map(|row| row.value.clone())
                .unwrap_or_default(),
        }
    }

    /// Writes the line's text to the book's details. Empty text removes it.
    pub fn write(self, session: &mut Session, value: &str) -> Result<(), String> {
        match self {
            Self::Title => session.set_field(Field::Title, value),
            Self::Subtitle => session.set_field(Field::Subtitle, value),
            Self::Author => session.set_field(Field::Authors, value),
            Self::Publisher if value.trim().is_empty() => session.remove_meta(PUBLISHER),
            Self::Publisher => session.set_meta(PUBLISHER, value),
        }
    }
}

pub enum TitlePageEvent {
    /// A line was changed and left.
    Write(Line, String),
    /// Escape: back to the Binder.
    Back,
}

impl EventEmitter<TitlePageEvent> for TitlePageView {}

pub struct TitlePageView {
    fields: Vec<(Line, Entity<TextInput>)>,
    book_type: Option<BookType>,
    zoom: f32,
    /// The pane in the window, read as it is painted, so the page centres on the window as the text does.
    pane: Rc<Cell<Bounds<Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

impl TitlePageView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        let fields = (Line::ALL.iter())
            .map(|&line| {
                let field = cx.new(|cx| {
                    TextInput::new(window, cx)
                        .bare()
                        .keep_focus_on_enter()
                        .with_label(line.name())
                        .with_placeholder(line.name())
                });
                subscriptions.extend([
                    cx.subscribe(&field, move |_, _, Committed(text), cx| {
                        cx.emit(TitlePageEvent::Write(line, text.clone()));
                    }),
                    cx.subscribe(&field, |_, _, _: &Edited, cx| cx.notify()),
                    cx.subscribe_in(&field, window, move |view, _, _: &Submitted, window, cx| {
                        view.step(line, true, window, cx);
                    }),
                    cx.subscribe(&field, |_, _, _: &Cancelled, cx| cx.emit(TitlePageEvent::Back)),
                ]);
                (line, field)
            })
            .collect();
        Self {
            fields,
            book_type: None,
            zoom: 1.,
            pane: Rc::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Shows what the book's details say now. A field being edited keeps its text.
    pub fn set_details(&mut self, details: &Details, title: &str, window: &Window, cx: &mut Context<Self>) {
        for (line, field) in &self.fields {
            let value = line.read(details, title);
            let editing = field.read(cx).focus_handle(cx).is_focused(window);
            if !editing && field.read(cx).text() != value {
                field.update(cx, |field, cx| field.set_value(value, *line == Line::Title, cx));
            }
        }
        cx.notify();
    }

    /// Where the pane stands until the page is first painted: the text's own pane, so the page opens in place.
    pub fn set_pane(&mut self, pane: Bounds<Pixels>) {
        if self.pane.get().size.width <= px(0.) {
            self.pane.set(pane);
        }
    }

    /// Sets the lines in the book's type at the text's zoom.
    pub fn set_type(&mut self, book_type: Option<BookType>, zoom: f32, cx: &mut Context<Self>) {
        if self.book_type == book_type && self.zoom == zoom {
            return;
        }
        self.book_type = book_type;
        self.zoom = zoom;
        for (line, field) in &self.fields {
            let style = self.style(*line);
            field.update(cx, |field, cx| field.set_book_type(Some(style), cx));
        }
        cx.notify();
    }

    /// The font and size of a line: the title as a chapter's title, the subtitle in italics, the author in small
    /// capitals, and the publisher small.
    fn style(&self, line: Line) -> (Font, Pixels) {
        let scale = tokens::POINT * self.zoom;
        let (family, body, title, weight) = match &self.book_type {
            Some(book) => (
                book.body.family.clone(),
                book.body.size,
                book.headings[0].size.max(book.body.size * 2.),
                book.headings[0].weight,
            ),
            None => (tokens::FONT_UI.to_owned(), 12., 24., 600),
        };
        let mut chosen = font(family);
        let size = match line {
            Line::Title => {
                chosen.weight = FontWeight(f32::from(weight));
                title
            }
            Line::Subtitle => {
                chosen.style = FontStyle::Italic;
                body * 1.1
            }
            Line::Author => {
                chosen.features = FontFeatures(Arc::new(vec![("smcp".into(), 1), ("c2sc".into(), 1)]));
                body
            }
            Line::Publisher => body * 0.85,
        };
        (chosen, px(size * scale))
    }

    /// Gives the keys to the title, with the caret after its last letter rather than over all of it.
    pub fn take_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.fields[0].1.clone();
        title.read(cx).focus_handle(cx).focus(window, cx);
        // A field selects all of its text as it gains the keys, which it hears while the next frame draws.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |_, cx| title.update(cx, |title, cx| title.set_caret(usize::MAX, cx)));
        });
    }

    /// Gives the keys to the line after or before `from`, around the ends.
    fn step(&mut self, from: Line, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let at = Line::ALL.iter().position(|line| *line == from).unwrap_or_default();
        let count = Line::ALL.len();
        let next = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.fields[next].1.read(cx).focus_handle(cx).focus(window, cx);
    }

    /// The line that has the keys.
    fn focused(&self, window: &Window, cx: &gpui::App) -> Option<Line> {
        (self.fields.iter())
            .find(|(_, field)| field.read(cx).focus_handle(cx).is_focused(window))
            .map(|(line, _)| *line)
    }

    /// How wide a line's field is: its text, or its name while empty, with room for the caret.
    fn width(&self, line: Line, text: &str, window: &Window) -> Pixels {
        let (chosen, size) = self.style(line);
        let shown: SharedString = if text.is_empty() { line.name() } else { text }.to_owned().into();
        let run = TextRun {
            len: shown.len(),
            font: chosen,
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(shown, size, &[run], None);
        shaped.width + tokens::INPUT_CARET_ROOM * 2.
    }
}

impl Focusable for TitlePageView {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.fields[0].1.read(cx).focus_handle(cx)
    }
}

impl Render for TitlePageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx).clone();
        let body = self.style(Line::Subtitle).1;
        let measure = self
            .book_type
            .as_ref()
            .map(|book| px(book.measure * tokens::POINT * self.zoom));
        // Centred on the window as the text column is, so the page stands where the text stood.
        let pane = self.pane.get();
        let widths: Vec<Pixels> = (self.fields.iter())
            .map(|(line, field)| self.width(*line, field.read(cx).text(), window))
            .collect();
        // A line wider than the measure, as a long title is, widens the column rather than scrolling out of sight.
        let widest = widths.iter().copied().fold(px(0.), Pixels::max);
        let placed = (pane.size.width > px(0.)).then(|| {
            let width = measure.unwrap_or(tokens::TEXT_MIN_WIDTH).max(widest);
            let column = Column::place(pane, width, false, window.viewport_size().width * 0.5);
            (column.text_left - pane.left(), column.text_width)
        });
        let space_before = |line: Line| match line {
            Line::Title => body * 3.,
            Line::Subtitle => body * 0.5,
            Line::Author => body * 1.5,
            Line::Publisher => body * 5.,
        };
        let lines: Vec<_> = (self.fields.iter().zip(widths))
            .map(|((line, field), width)| {
                div()
                    .mt(space_before(*line))
                    .w(width)
                    .max_w_full()
                    .mx_auto()
                    .child(field.clone())
            })
            .collect();
        let caption = div()
            .flex()
            .items_center()
            .gap(tokens::SPACE_2)
            .text_size(tokens::TEXT_SMALL)
            .text_color(tokens.text_faint)
            .child(
                svg()
                    .flex_none()
                    .path("icons/title.svg")
                    .size(tokens::CHECK_SMALL)
                    .text_color(tokens.text_faint),
            )
            .child(crate::t!(Key::TitlePageCaption));
        div()
            .id("title-page")
            .key_context("TitlePage")
            .size_full()
            .overflow_y_scroll()
            .on_action(cx.listener(|view, _: &FocusNext, window, cx| {
                if let Some(line) = view.focused(window, cx) {
                    view.step(line, true, window, cx);
                }
            }))
            .on_action(cx.listener(|view, _: &FocusPrevious, window, cx| {
                if let Some(line) = view.focused(window, cx) {
                    view.step(line, false, window, cx);
                }
            }))
            .child({
                let pane = self.pane.clone();
                let view = cx.entity().downgrade();
                canvas(
                    move |bounds, _, _| bounds,
                    move |_, bounds: Bounds<Pixels>, _, cx| {
                        let before = pane.get();
                        if (before.left(), before.size.width) != (bounds.left(), bounds.size.width) {
                            pane.set(bounds);
                            view.update(cx, |_, cx| cx.notify()).ok();
                        }
                    },
                )
                .absolute()
                .size_full()
            })
            .child(
                div()
                    .pt(tokens::TEXT_MARGIN_Y)
                    .pb(tokens::TEXT_MARGIN_Y)
                    .map(|column| match placed {
                        Some((left, width)) => column.ml(left).w(width),
                        None => column
                            .mx_auto()
                            .px(tokens::SPACE_4)
                            .when_some(measure, |column, measure| column.w(measure).max_w_full()),
                    })
                    .flex()
                    .flex_col()
                    .child(caption)
                    .children(lines),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_line_writes_the_book_details() {
        let file = std::env::temp_dir().join(format!("druck-title-page-{}.md", std::process::id()));
        std::fs::write(&file, "# One\n").expect("file");
        let mut session = Session::open(&file).expect("opens");
        Line::Title.write(&mut session, "The Keeper").expect("title");
        Line::Subtitle.write(&mut session, "A novel").expect("subtitle");
        Line::Author.write(&mut session, "Mara Ellison").expect("author");
        Line::Publisher.write(&mut session, "Harbour Press").expect("publisher");
        let details = session.details().clone();
        let read: Vec<String> = Line::ALL
            .iter()
            .map(|line| line.read(&details, &details.title))
            .collect();
        assert_eq!(read, ["The Keeper", "A novel", "Mara Ellison", "Harbour Press"]);
        Line::Publisher.write(&mut session, "").expect("removes");
        assert!(session.details().meta.is_empty());
        assert!(
            session
                .joined()
                .text
                .starts_with("---\ntitle: The Keeper\nsubtitle: A novel\n")
        );
        std::fs::remove_file(&file).ok();
    }
}
