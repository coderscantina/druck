//! A starter as a page: its sample made as a project, laid out by the engine, and painted as Publish paints pages.
//! Holds no gpui types.

use std::fs;
use std::path::PathBuf;

use druck::cancel::Cancel;
use resvg::tiny_skia::Pixmap;

use super::job::Typesetter;
use crate::session::{Session, Starter};
use crate::shell::strings::{Language, Table};

/// The folder the samples are made in, which is this process's own and is removed after each page.
fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("druck-gallery-{}", std::process::id()))
}

/// The page of `starter`'s sample that the gallery shows, `width` pixels wide, in `language`.
pub fn page(starter: &Starter, language: Language, width: u32) -> Result<Pixmap, String> {
    let dir = scratch().join(starter.slug).join(Table::text(starter.folder, language));
    fs::remove_dir_all(&dir).ok();
    let painted = paint(starter, &dir, language, width);
    fs::remove_dir_all(scratch().join(starter.slug)).ok();
    fs::remove_dir(scratch()).ok();
    painted
}

fn paint(starter: &Starter, dir: &std::path::Path, language: Language, width: u32) -> Result<Pixmap, String> {
    starter.create(dir, language)?;
    let session = Session::open(dir)?;
    let layout = Typesetter::new(session.kind().clone(), None)
        .run(session.joined(), &Cancel::default())
        .map_err(|failure| failure.to_string())?;
    let page = (layout.laid.pages().get(starter.shown)).ok_or_else(|| format!("{} has no such page", starter.slug))?;
    Ok(layout.painter.paint(page, width as f32 / page.width.0 as f32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::STARTERS;

    #[test]
    fn every_starter_paints_the_page_it_shows_in_both_languages() {
        for starter in STARTERS {
            for language in [Language::English, Language::German] {
                let page = page(starter, language, 120).unwrap_or_else(|problem| panic!("{}: {problem}", starter.slug));
                assert_eq!(page.width(), 120, "{}", starter.slug);
                let inked = page.pixels().iter().filter(|pixel| pixel.red() < 230).count();
                assert!(
                    inked > 100,
                    "{} shows a page with text, {inked} inked pixels",
                    starter.slug
                );
            }
        }
    }
}
