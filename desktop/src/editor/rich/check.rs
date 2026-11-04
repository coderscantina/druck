//! Whether a rich edit leaves valid Druck. The engine's parser reads the whole book with the edit applied, and an edit
//! that brings a problem not there before is refused with the engine's words. A book that already has problems can still
//! be edited.

use druck::editor::resolved_config::split_front_matter;
use druck::editor::rich::parse_problems;

use crate::editor::commands::applied;
use crate::session::{ChapterId, Session};
use crate::source::Edit;

fn language(session: &Session) -> &'static str {
    if session.details().language == "de" { "de" } else { "en" }
}

/// The document's Markdown as the parser reads it, with `change` applied to one chapter.
fn body_with(session: &Session, change: Option<(ChapterId, &[Edit])>) -> String {
    let mut joined = String::new();
    let in_book = change.is_none_or(|(id, _)| session.book().contains(&id));
    let chapters: Vec<ChapterId> = if in_book {
        session.book().to_vec()
    } else {
        change.iter().map(|(id, _)| *id).collect()
    };
    for (index, id) in chapters.iter().enumerate() {
        let Some(chapter) = session.chapter(*id) else {
            continue;
        };
        if index > 0 {
            joined.push_str("\n\n");
        }
        match change {
            Some((changed, edits)) if changed == *id => joined.push_str(&applied(chapter.text(), edits)),
            _ => joined.push_str(chapter.text()),
        }
    }
    match split_front_matter(&joined) {
        Ok(Some(split)) => split.body.to_owned(),
        _ => joined,
    }
}

/// What the parser says about the document now, for comparing against.
pub fn problems_now(session: &Session) -> Vec<String> {
    parse_problems(&body_with(session, None), language(session))
}

/// Why these edits to a chapter cannot be written, given the document's problems before them.
pub fn refusal(session: &Session, chapter: ChapterId, edits: &[Edit], before: &[String]) -> Option<String> {
    let after = parse_problems(&body_with(session, Some((chapter, edits))), language(session));
    after.into_iter().find(|problem| !before.contains(problem))
}
