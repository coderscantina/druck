//! The small pieces the versions views share, built from the tokens.

use gpui::prelude::*;
use gpui::{
    ElementId, Font, FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, Stateful, StrikethroughStyle,
    StyledText, UnderlineStyle, div,
};

use super::shown::{self, Run, Tint};
use crate::a11y::Control as _;
use crate::tokens::{self, Tokens};

/// The small label at the top of a card or panel, on a divider.
pub fn section_label(tokens: &Tokens, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .h(tokens::SECTION_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tokens.text_quiet)
        .child(label.into())
}

/// How a stretch of shown text differs: the timeline's tints, with the removed words struck through.
pub fn run_highlight(run: &Run, marked: bool, tokens: &Tokens) -> HighlightStyle {
    let tint = run.tint.map(|tint| match tint {
        Tint::Removed => Hsla::from(tokens.removed),
        Tint::Added => Hsla::from(tokens.added),
    });
    HighlightStyle {
        background_color: tint.map(|tint| tint.opacity(tokens::DIFF_TINT)),
        strikethrough: (run.tint == Some(Tint::Removed)).then_some(StrikethroughStyle {
            thickness: tokens::HAIRLINE,
            color: None,
        }),
        underline: marked.then(|| UnderlineStyle {
            thickness: tokens::HAIRLINE,
            color: Some(tokens.accent.into()),
            wavy: false,
        }),
        font_weight: run.bold.then_some(FontWeight::BOLD),
        font_style: run.italic.then_some(FontStyle::Italic),
        ..Default::default()
    }
}

/// What the other side wrote against the author's words, in the book's type with the timeline's tints: the author's words
/// that go are struck through, and the other side's are tinted as added.
pub fn compared(mine: &str, theirs: &str, font: &Font, tokens: &Tokens) -> Vec<gpui::AnyElement> {
    (shown::build(mine, theirs).blocks.iter())
        .map(|block| {
            let highlights: Vec<_> = (block.runs.iter())
                .filter(|run| run.tint.is_some() || run.bold || run.italic)
                .map(|run| (run.range.clone(), run_highlight(run, false, tokens)))
                .collect();
            div()
                .font(font.clone())
                .text_size(tokens::SLIP_TEXT)
                .text_color(tokens.text)
                .child(StyledText::new(SharedString::from(block.text.clone())).with_highlights(highlights))
                .into_any_element()
        })
        .collect()
}

/// A quiet group label in a panel.
pub fn group_label(tokens: &Tokens, label: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .pt(tokens::SPACE_3)
        .pb(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_faint)
        .child(label.into())
}

/// A 22 px button with a thin border and quiet text. Nothing is filled.
pub fn button(tokens: &Tokens, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    let label = label.into();
    div()
        .id(id)
        .button(tokens, label.clone())
        .flex_none()
        .h(tokens::VERSIONS_BUTTON_HEIGHT)
        .px(tokens::SPACE_2)
        .flex()
        .items_center()
        .rounded(tokens::SETTINGS_SWATCH_RADIUS)
        .border_1()
        .border_color(tokens.divider)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .cursor_pointer()
        .hover(|button| button.bg(tokens.raised))
        .child(label)
}

/// A file's or commit's row in a list of changes: its title, and quiet words at the end.
pub fn change_row(tokens: &Tokens, title: String, words: String) -> gpui::Div {
    let (lead, title) = split_lead(&title);
    div()
        .flex_none()
        .min_h(tokens::VERSIONS_ROW_HEIGHT)
        .px(tokens::SPACE_3)
        .flex()
        .items_center()
        .gap(tokens::SPACE_2)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .children(lead.map(|lead| div().flex_none().child(lead.to_owned())))
        .child(div().flex_1().min_w_0().truncate().child(title.to_owned()))
        .child(div().flex_none().text_color(tokens.text_faint).child(words))
}

/// A leading symbol, such as a commit's gitmoji, and the rest. An emoji drawn wider than it measures, as `✏️` is, would
/// push a cut title's ellipsis out of its box, so it stands apart from the text that is cut.
fn split_lead(title: &str) -> (Option<&str>, &str) {
    let Some((first, rest)) = title.split_once(' ') else {
        return (None, title);
    };
    let symbol = !first.is_empty() && first.chars().all(|c| !c.is_ascii() && !c.is_alphanumeric());
    if symbol { (Some(first), rest) } else { (None, title) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_symbol_stands_apart_from_the_title() {
        assert_eq!(
            split_lead("\u{270f}\u{fe0f} Michael's own pass"),
            (Some("\u{270f}\u{fe0f}"), "Michael's own pass")
        );
        assert_eq!(split_lead("Add a sentence"), (None, "Add a sentence"));
        assert_eq!(split_lead("Über den Autor"), (None, "Über den Autor"));
    }
}

/// A line of text that says something is off, or what to do next, in the accent colour.
pub fn accent_line(tokens: &Tokens, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .py(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.accent)
        .child(text.into())
}

/// A quiet line of explanation.
pub fn quiet_line(tokens: &Tokens, text: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex_none()
        .px(tokens::SPACE_3)
        .py(tokens::SPACE_1)
        .text_size(tokens::TEXT_SMALL)
        .text_color(tokens.text_quiet)
        .child(text.into())
}
