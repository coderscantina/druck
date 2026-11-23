//! The pages Druck makes, contents, index, and bibliography, shown in Write as the page they make and quieter than the
//! writing: the page's title, its live entries, and a caption saying Druck writes it. The page numbers come from the
//! last Publish layout, and so do the page's title and the contents' depth, as the theme sets them. Before the first one
//! the entries show without numbers and the title in the app's words, so nothing pops in empty, and a new layout only
//! changes numbers: how many rows show follows the text and the depth alone, so the text around never moves.
//!
//! A section that holds nothing but such a page shows it larger, with the caption at the top and Remove beside it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use druck::output::{ChapterId as MapChapter, Layout, SourcePos};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Font, FontFeatures, Hsla, MouseButton, Pixels, Point, ShapedLine, SharedString,
    TextAlign, TextRun, TransformationMatrix, Window, div, fill, font, point, px, size,
};

use super::EditorView;
use super::directives::Kind;
use super::strings::Key;
use crate::session::RowKind;
use crate::session::outline::heading_text;
use crate::source::{BlockKind, Blocks};
use crate::tokens::{self, Tokens};

/// The most entries a made page shows. The rest are counted on one more row.
const MAX_ENTRIES: usize = 12;
/// The heading levels the contents list before a layout says the theme's: the default theme's `toc-depth`.
const DEPTH_BEFORE_LAYOUT: u8 = 2;
/// An entry's row, in body lines.
const ENTRY_LINES: f32 = 1.2;
/// The room between the title, the entries, and the caption, in body sizes.
const GAP_EMS: f32 = 0.7;
/// The space between two leader dots.
const DOT_STEP: Pixels = px(4.);

/// One row of a made page: its words and the page they are printed on, once a layout says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Entry {
    pub words: String,
    pub page: Option<String>,
}

/// The rows a made page shows: at most [`MAX_ENTRIES`], then how many more there are.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct Entries {
    pub shown: Vec<Entry>,
    pub more: usize,
}

impl Entries {
    fn capped(mut all: Vec<Entry>) -> Self {
        let more = all.len().saturating_sub(MAX_ENTRIES);
        all.truncate(MAX_ENTRIES);
        Self { shown: all, more }
    }

    /// The rows it takes, with the count of the rest.
    pub fn rows(&self) -> usize {
        self.shown.len() + usize::from(self.more > 0)
    }
}

/// A heading as the last layout printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Printed {
    pub level: u8,
    pub title: String,
    pub page: Option<String>,
}

/// A heading of the book as its text writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Heading {
    pub level: u8,
    pub title: String,
    /// It is not `{.unlisted}`.
    pub listed: bool,
}

/// What the last layout says about the made pages, and the entries read from the book as it stands.
#[derive(Default)]
pub(super) struct Made {
    headings: Vec<Printed>,
    /// By index entry, the pages its marks are printed on, in page order.
    index: HashMap<String, Vec<String>>,
    /// The deepest heading level the contents list, once a layout says.
    depth: Option<u8>,
    /// Changes with each layout taken, so the entries are read again.
    generation: u64,
    cache: RefCell<Vec<Cached>>,
}

/// A made page's entries as read for a revision of the book and a layout.
struct Cached {
    kind: Kind,
    revision: u64,
    generation: u64,
    entries: Rc<Entries>,
}

/// The number printed on page `page`, from zero. A book without parts shows each page's place, and a page before the
/// first part shows none.
pub(super) fn printed_page(layout: &Layout, page: usize) -> Option<String> {
    let numbers = layout.page_numbers();
    if numbers.is_empty() {
        return Some((page + 1).to_string());
    }
    let number = numbers.get(page).copied().flatten()?;
    Some(number.format.format(number.value))
}

/// The layout's headings in book order, each with the number of the page it is printed on.
pub(super) fn printed_headings(layout: &Layout) -> Vec<Printed> {
    (layout.outline().iter())
        .map(|bookmark| Printed {
            level: bookmark.level,
            title: bookmark.title.clone(),
            page: printed_page(layout, layout.anchor(bookmark.anchor).page),
        })
        .collect()
}

/// The headings `text` writes, in reading order.
pub(super) fn headings(text: &str) -> Vec<Heading> {
    let blocks = Blocks::parse(text);
    (blocks.as_slice().iter())
        .filter_map(|block| {
            let BlockKind::Heading { level } = block.kind else {
                return None;
            };
            let line = text[block.range.clone()].lines().next().unwrap_or_default();
            let (title, _) = heading_text(line);
            let attributes = line.trim_end().strip_suffix('}').and_then(|line| line.rsplit_once('{'));
            let unlisted = attributes.is_some_and(|(_, inside)| inside.split_whitespace().any(|a| a == ".unlisted"));
            Some(Heading {
                level,
                title,
                listed: !unlisted,
            })
        })
        .collect()
}

/// The letters and digits of a title, so a typographic quote on the page still matches a straight one in the text.
fn folded(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The contents' entries: the listed headings down to `depth`, each with the page the layout printed it on.
/// A heading is matched to the next printed one of its level whose title ends with its own, since the layout's has its
/// number before it, so one the layout lacks keeps no number and the rest stay in step.
pub(super) fn contents(headings: &[Heading], printed: &[Printed], depth: u8) -> Vec<Entry> {
    let mut next = 0;
    let mut entries = Vec::new();
    for heading in headings {
        let title = folded(&heading.title);
        let found = (printed[next..].iter())
            .position(|other| other.level == heading.level && folded(&other.title).ends_with(&title))
            .map(|at| next + at);
        let page = found.and_then(|at| printed[at].page.clone());
        if let Some(at) = found {
            next = at + 1;
        }
        if heading.listed && heading.level <= depth {
            entries.push(Entry {
                words: heading.title.clone(),
                page,
            });
        }
    }
    entries
}

/// The index's main entries in the order the page sorts them, each with the pages of its marks.
fn index_entries(terms: impl Iterator<Item = String>, pages: &HashMap<String, Vec<String>>) -> Vec<Entry> {
    let mut words: Vec<String> = terms.collect();
    words.sort_by_key(|word| word.to_lowercase());
    words.dedup_by_key(|word| word.to_lowercase());
    (words.into_iter())
        .map(|words| Entry {
            page: pages
                .get(&words)
                .filter(|pages| !pages.is_empty())
                .map(|pages| pages.join(", ")),
            words,
        })
        .collect()
}

/// What a made page draws, shaped for one frame, with places from its block's top left.
pub(in crate::editor) struct MadeShape {
    texts: Vec<(ShapedLine, Point<Pixels>, Pixels)>,
    dots: Vec<Bounds<Pixels>>,
    icon: Bounds<Pixels>,
    faint: Hsla,
}

/// The sizes of a made page at this zoom.
struct Metrics {
    title: Font,
    title_size: Pixels,
    title_line: Pixels,
    entry_line: Pixels,
    gap: Pixels,
    caption: Pixels,
}

impl EditorView {
    /// Takes what a fresh layout printed: the headings' and index marks' page numbers, the contents' depth, and the
    /// made pages' titles. New titles rename the made pages' rows, which the session says to every view.
    pub fn take_layout(&mut self, layout: &Layout, cx: &mut Context<Self>) {
        let headings = printed_headings(layout);
        let index = self.printed_index(layout);
        let depth = Some(layout.contents_depth());
        let renamed = self.session.set_page_titles(layout.page_titles());
        if headings != self.made.headings || index != self.made.index || depth != self.made.depth {
            self.made.headings = headings;
            self.made.index = index;
            self.made.depth = depth;
            self.made.generation += 1;
            cx.notify();
        }
        if renamed {
            cx.emit(super::SessionChanged);
            cx.notify();
        }
    }

    /// The title of the made page of `kind`: the theme's from the last layout, else the app's words.
    fn made_title(&self, kind: Kind) -> &str {
        let row = match kind {
            Kind::Toc => RowKind::Contents,
            Kind::Index => RowKind::Index,
            _ => RowKind::Bibliography,
        };
        self.session.page_title(row).unwrap_or_else(|| kind.label())
    }

    /// The pages each index entry's marks are printed on, found from where the marks are written.
    fn printed_index(&self, layout: &Layout) -> HashMap<String, Vec<String>> {
        let mut pages: HashMap<String, Vec<(usize, String)>> = HashMap::new();
        let Some(map) = layout.source_map() else {
            return HashMap::new();
        };
        let session = &self.session;
        for id in session.book() {
            let Some(chapter) = session.chapter(*id) else {
                continue;
            };
            for (offset, entry) in super::rich::index::entries(chapter.text()) {
                let Some((file, at)) = session.file_position(*id, offset) else {
                    continue;
                };
                let Some(chapter) = map.chapters().iter().position(|chapter| chapter.path == file) else {
                    continue;
                };
                let at = SourcePos {
                    chapter: MapChapter(chapter as u32),
                    offset: at as u32,
                };
                let Some(page) = layout.locate(at).map(|located| located.page) else {
                    continue;
                };
                let Some(label) = printed_page(layout, page) else {
                    continue;
                };
                let found = pages.entry(entry).or_default();
                if !found.iter().any(|(other, _)| *other == page) {
                    found.push((page, label));
                }
            }
        }
        (pages.into_iter())
            .map(|(entry, mut found)| {
                found.sort_by_key(|(page, _)| *page);
                (entry, found.into_iter().map(|(_, label)| label).collect())
            })
            .collect()
    }

    /// The entries of the made page of `kind`, read again when the book or the layout changed.
    pub(super) fn made_entries(&self, kind: Kind) -> Rc<Entries> {
        let revision = self.session.revision();
        let generation = self.made.generation;
        if let Some(cached) = (self.made.cache.borrow().iter())
            .find(|cached| cached.kind == kind && cached.revision == revision && cached.generation == generation)
        {
            return cached.entries.clone();
        }
        let session = &self.session;
        let texts = || (session.book().iter()).filter_map(|id| session.chapter(*id).map(|chapter| chapter.text()));
        let all = match kind {
            Kind::Toc => {
                let headings: Vec<Heading> = texts().flat_map(headings).collect();
                let depth = self.made.depth.unwrap_or(DEPTH_BEFORE_LAYOUT);
                contents(&headings, &self.made.headings, depth)
            }
            Kind::Index => index_entries(
                texts().flat_map(|text| super::rich::index::entries(text).into_iter().map(|(_, entry)| entry)),
                &self.made.index,
            ),
            _ => self.cited_works(texts()),
        };
        let entries = Rc::new(Entries::capped(all));
        let mut cache = self.made.cache.borrow_mut();
        cache.retain(|cached| cached.kind != kind);
        cache.push(Cached {
            kind,
            revision,
            generation,
            entries: entries.clone(),
        });
        entries
    }

    /// The works the book cites, in the bibliography's order, as their authors and title. The engine's own formatting
    /// is not at hand in Write.
    fn cited_works<'a>(&self, texts: impl Iterator<Item = &'a str>) -> Vec<Entry> {
        let texts: Vec<&str> = texts.collect();
        let cites = |key: &str| {
            texts.iter().any(|text| {
                text.match_indices(&format!("@{key}")).any(|(at, written)| {
                    let after = text[at + written.len()..].chars().next();
                    !after.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':'))
                })
            })
        };
        (self.rich.works.iter())
            .filter(|work| cites(&work.key))
            .map(|work| Entry {
                words: [work.authors.as_str(), work.title.as_str()]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(", "),
                page: None,
            })
            .collect()
    }

    /// Whether the active section holds nothing but directives, so its made page shows as the whole page.
    pub(super) fn made_alone(&self) -> bool {
        self.directives.structure.is_made_page()
    }

    fn made_metrics(&self, alone: bool) -> Metrics {
        let level = if alone { 1 } else { 2 };
        let title = self.style.line_style(BlockKind::Heading { level });
        let body_line = self.style.body_size * self.style.leading;
        Metrics {
            title: title.font,
            title_size: title.size,
            title_line: title.size * title.leading,
            entry_line: body_line * ENTRY_LINES,
            gap: self.style.body_size * GAP_EMS,
            caption: if alone {
                tokens::SMALL_BUTTON_HEIGHT
            } else {
                tokens::TEXT_SMALL * 1.5
            },
        }
    }

    /// The room the made page of `kind` takes below its line. It changes only when its count of rows does.
    pub(super) fn made_room(&self, kind: Kind) -> Pixels {
        let alone = self.made_alone();
        let metrics = self.made_metrics(alone);
        let entries = metrics.entry_line * self.made_entries(kind).rows() as f32;
        metrics.title_line + metrics.caption + entries + metrics.gap * 2.
    }

    /// The made page of `kind`, shaped for a block `width` wide, faded as a ghost.
    pub(in crate::editor) fn made_shape(
        &self,
        kind: Kind,
        width: Pixels,
        ghost: bool,
        window: &Window,
        cx: &App,
    ) -> MadeShape {
        let colors = Tokens::get(cx);
        let opacity = if ghost { tokens::GHOST_OPACITY } else { 1. };
        let quiet = self.style.quiet.opacity(opacity);
        let faint: Hsla = Hsla::from(colors.text_faint).opacity(opacity);
        let alone = self.made_alone();
        let metrics = self.made_metrics(alone);
        let entries = self.made_entries(kind);
        let body = self.style.body.clone();
        let figures = Font {
            features: FontFeatures(Arc::new(vec![("onum".into(), 1)])),
            ..body.clone()
        };
        let shape = |text: &str, font: &Font, size: Pixels, color: Hsla| {
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(SharedString::from(text.to_owned()), size, &[run], None)
        };
        let ui = font(tokens::FONT_UI);
        let mut texts = Vec::new();
        let mut dots = Vec::new();
        let caption = shape(made_caption(kind), &ui, tokens::TEXT_SMALL, faint);
        let icon_size = tokens::NOTE_ICON;
        let mut y = px(0.);
        let caption_at = |y: Pixels| {
            let middle = y + metrics.caption * 0.5;
            (
                Bounds::new(point(px(0.), middle - icon_size * 0.5), size(icon_size, icon_size)),
                point(icon_size + tokens::SPACE_2, middle - tokens::TEXT_SMALL * 0.75),
            )
        };
        let mut icon = Bounds::default();
        if alone {
            let (at, words) = caption_at(y);
            icon = at;
            texts.push((caption.clone(), words, tokens::TEXT_SMALL * 1.5));
            y += metrics.caption + metrics.gap;
        }
        let title = shape(self.made_title(kind), &metrics.title, metrics.title_size, quiet);
        texts.push((title, point(px(0.), y), metrics.title_line));
        y += metrics.title_line + metrics.gap;
        let size_now = self.style.body_size;
        for entry in &entries.shown {
            let page = entry.page.as_deref().map(|page| shape(page, &figures, size_now, quiet));
            let page_width = page.as_ref().map_or(px(0.), |page| page.width);
            let room = width - page_width - tokens::SPACE_4 * 2.;
            let words = shape(
                &fitted(&entry.words, room, |text| shape(text, &body, size_now, quiet).width),
                &body,
                size_now,
                quiet,
            );
            let baseline = y + metrics.entry_line * 0.5 + size_now * 0.3;
            let (from, to) = (words.width + tokens::SPACE_2, width - page_width - tokens::SPACE_2);
            if page.is_some() {
                let mut x = from + (DOT_STEP - (from % DOT_STEP));
                while x + tokens::HAIRLINE <= to {
                    dots.push(Bounds::new(
                        point(x, baseline),
                        size(tokens::HAIRLINE, tokens::HAIRLINE),
                    ));
                    x += DOT_STEP;
                }
            }
            texts.push((words, point(px(0.), y), metrics.entry_line));
            if let Some(page) = page {
                let x = width - page.width;
                texts.push((page, point(x, y), metrics.entry_line));
            }
            y += metrics.entry_line;
        }
        if entries.more > 0 {
            let more = crate::t!(Key::MadeMore, count = entries.more);
            texts.push((
                shape(&more, &body, size_now, faint),
                point(px(0.), y),
                metrics.entry_line,
            ));
            y += metrics.entry_line;
        }
        if !alone {
            let (at, words) = caption_at(y + metrics.gap);
            icon = at;
            texts.push((caption, words, tokens::TEXT_SMALL * 1.5));
        }
        MadeShape {
            texts,
            dots,
            icon,
            faint,
        }
    }

    /// Remove beside the caption of a made page standing alone, at the right of the text.
    pub(super) fn made_buttons(&self, cx: &mut Context<Self>) -> Vec<(AnyElement, Point<Pixels>)> {
        if self.source || !self.made_alone() {
            return Vec::new();
        }
        let tokens_now = Tokens::get(cx).clone();
        let structure = &self.directives.structure;
        let mut buttons = Vec::new();
        for (index, top) in &self.visible {
            if !self.standalone_at_rest(*index).is_some_and(is_made) {
                continue;
            }
            let line = self.blocks.as_slice()[*index].range.start;
            let Some(region) = structure.fence(*index) else {
                continue;
            };
            let row = div()
                .w(self.column.text_width)
                .flex()
                .justify_end()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    crate::shell::ui::small_button(&tokens_now, ("made-remove", region), crate::t!(Key::MadeRemove))
                        .on_click(cx.listener(move |this, _, window, cx| this.remove_made(line, window, cx))),
                );
            buttons.push((row.into_any_element(), point(self.column.text_left, *top)));
        }
        buttons
    }

    /// Takes the made page whose line starts at `line` out of the text, as one undo step.
    fn remove_made(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.blocks.index_at(line);
        let structure = &self.directives.structure;
        let Some(region) = structure
            .fence(index)
            .filter(|&region| is_made(structure.regions[region].kind))
        else {
            return;
        };
        let text = self.session.active_buffer().text();
        let command = super::directives::edits::delete_region(text, &self.blocks, structure, region);
        if let Some(command) = command {
            self.run_command(command, window, cx);
        }
    }
}

/// Whether Druck writes this page itself.
pub(super) fn is_made(kind: Kind) -> bool {
    matches!(kind, Kind::Toc | Kind::Index | Kind::Bibliography)
}

fn made_caption(kind: Kind) -> &'static str {
    crate::t!(match kind {
        Kind::Index => Key::MadeIndex,
        Kind::Bibliography => Key::MadeBibliography,
        _ => Key::MadeContents,
    })
}

/// `words` cut at the last whole word that fits `room`, with an ellipsis, as `width` measures them.
fn fitted(words: &str, room: Pixels, width: impl Fn(&str) -> Pixels) -> String {
    if width(words) <= room {
        return words.to_owned();
    }
    let mut cut = words;
    while let Some((before, _)) = cut.rsplit_once(' ') {
        cut = before;
        let tried = format!("{cut}\u{2026}");
        if width(&tried) <= room {
            return tried;
        }
    }
    format!("{cut}\u{2026}")
}

/// Paints a made page with its top left at `origin`.
pub(in crate::editor) fn paint_made(shape: &MadeShape, origin: Point<Pixels>, window: &mut Window, cx: &mut App) {
    for dot in &shape.dots {
        window.paint_quad(fill(Bounds::new(origin + dot.origin, dot.size), shape.faint));
    }
    window
        .paint_svg(
            Bounds::new(origin + shape.icon.origin, shape.icon.size),
            "icons/made.svg".into(),
            None,
            TransformationMatrix::unit(),
            shape.faint,
            cx,
        )
        .ok();
    for (line, at, height) in &shape.texts {
        line.paint(origin + *at, *height, TextAlign::Left, None, window, cx)
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use druck::cancel::Cancel;

    use super::*;
    use crate::preview::job::Typesetter;
    use crate::session::{Joined, Kind as SessionKind};

    #[test]
    fn contents_take_the_printed_numbers_of_listed_headings_to_the_layouts_depth() {
        let text = "::: front-matter\n\n# Preface {-}\n\nWords.\n\n::: toc\n\n::: main-matter\n\n# The arrival\n\nText.\n\n## A part of it\n\nMore.\n\n::: page-break\n\n# The relief {.unlisted}\n\nText.\n\n# It's over\n\nEnd.\n";
        let path = std::env::temp_dir().join("druck-made-probe.md");
        let laid = Typesetter::new(SessionKind::Single { path }, None)
            .run(Joined::single(text.to_owned(), 0), &Cancel::default())
            .unwrap_or_else(|_| panic!("the text typesets"))
            .laid;
        assert_eq!(laid.contents_depth(), 2, "the default theme's");
        let entries = contents(&headings(text), &printed_headings(&laid), laid.contents_depth());
        let shown: Vec<(&str, Option<&str>)> = (entries.iter())
            .map(|entry| (entry.words.as_str(), entry.page.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Preface", Some("i")),
                ("The arrival", Some("1")),
                ("A part of it", Some("1")),
                ("It's over", Some("2"))
            ]
        );
    }

    #[test]
    fn before_a_layout_the_entries_have_no_numbers_and_the_same_rows() {
        let headings = headings("# One\n\n# Two {.unlisted}\n\n# Three\n");
        let entries = contents(&headings, &[], 1);
        assert_eq!(
            entries,
            [
                Entry {
                    words: "One".into(),
                    page: None
                },
                Entry {
                    words: "Three".into(),
                    page: None
                }
            ]
        );
    }

    #[test]
    fn a_long_list_shows_twelve_and_counts_the_rest() {
        let all = (0..20)
            .map(|at| Entry {
                words: at.to_string(),
                page: None,
            })
            .collect();
        let entries = Entries::capped(all);
        assert_eq!((entries.shown.len(), entries.more, entries.rows()), (12, 8, 13));
    }

    #[test]
    fn index_entries_are_sorted_once_each_with_their_pages() {
        let pages = HashMap::from([("lamp".to_owned(), vec!["1".to_owned(), "4".to_owned()])]);
        let entries = index_entries(["lamp", "Boat", "lamp"].into_iter().map(str::to_owned), &pages);
        let shown: Vec<(&str, Option<&str>)> = (entries.iter())
            .map(|entry| (entry.words.as_str(), entry.page.as_deref()))
            .collect();
        assert_eq!(shown, [("Boat", None), ("lamp", Some("1, 4"))]);
    }
}
