//! Which blocks of a document use a theme style, so an edit to the style can show where it matters. The source map
//! gives each block's kind and place; the style follows from the kind, the blocks around it, and a `{.name}` written
//! at its end. No engine change is needed for this. Holds no gpui types.

use std::ops::Range;
use std::path::Path;

use druck::output::{BlockKind, BlockSource, SourceMap, SourcePos};

/// Styles of text that is not in any block: the title block, running heads, and the watermark. They are on the first
/// page of any document that has them.
const PAGE_STYLES: &[&str] = &[
    "title",
    "subtitle",
    "author",
    "date",
    "abstract-heading",
    "abstract",
    "header",
    "footer",
    "watermark",
];

/// Where to look at a style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A block, by the place it starts.
    Block(SourcePos),
    /// The first page, for text outside every block.
    FirstPage,
}

/// The name in a `{.name}` that ends a block's text.
fn class_of(text: &str) -> Option<&str> {
    let text = text.trim_end();
    let inner = text.strip_suffix('}')?;
    let name = inner[inner.rfind("{.")? + 2..].trim();
    (!name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).then_some(name)
}

/// The styles a block is set in. `parents` are the blocks around it, outermost first, and `text` is the block as
/// written.
pub fn styles_of(kind: BlockKind, parents: &[BlockKind], text: &str) -> Vec<String> {
    let own = |style: &str| vec![style.to_owned()];
    let named = |style: String| match class_of(text) {
        Some(class) => vec![style, class.to_owned()],
        None => vec![style],
    };
    match kind {
        BlockKind::Heading(level) => named(format!("heading-{level}")),
        BlockKind::Paragraph => {
            let style = parents
                .iter()
                .rev()
                .find_map(|parent| match parent {
                    BlockKind::Quote => Some("quote"),
                    BlockKind::Callout => Some("callout"),
                    BlockKind::ListItem | BlockKind::List => Some("list"),
                    BlockKind::Footnote => Some("footnote"),
                    BlockKind::Table => Some("table-cell"),
                    _ => None,
                })
                .unwrap_or("body");
            named(style.to_owned())
        }
        BlockKind::List | BlockKind::ListItem => own("list"),
        BlockKind::Quote => own("quote"),
        BlockKind::Callout => vec!["callout".to_owned(), "callout-label".to_owned()],
        BlockKind::Code => own("code-block"),
        BlockKind::Image => own("caption"),
        BlockKind::Table => vec!["table-cell".to_owned(), "table-header".to_owned(), "caption".to_owned()],
        BlockKind::Footnote => own("footnote"),
        BlockKind::SceneBreak => own("scene-break"),
        BlockKind::Directive => match directive(text) {
            Some("toc") => vec!["toc-heading".to_owned(), "toc-entry".to_owned()],
            Some("index") => vec![
                "index-letter".to_owned(),
                "index-entry".to_owned(),
                "index-sub-entry".to_owned(),
            ],
            Some("bibliography") => own("bibliography"),
            _ => Vec::new(),
        },
    }
}

/// The name of a `::: name` line.
fn directive(text: &str) -> Option<&str> {
    let line = text.lines().next()?.trim_start().strip_prefix(":::")?;
    line.split_whitespace().next()
}

/// The first block that uses a style, or the first page for styles outside blocks. Blocks in `prefer`'s file come
/// first, since that is where the author is. `text_of` gives a block's text as written, by file and byte range.
pub fn find(
    map: &SourceMap,
    style: &str,
    prefer: Option<&Path>,
    text_of: &dyn Fn(&Path, Range<usize>) -> Option<String>,
) -> Option<Target> {
    if PAGE_STYLES.contains(&style) {
        return Some(Target::FirstPage);
    }
    let blocks = map.blocks();
    let chapters = map.chapters();
    let preferred = |block: &&BlockSource| {
        prefer.is_some_and(|file| {
            chapters
                .get(block.range.0.chapter.0 as usize)
                .is_some_and(|chapter| chapter.path == file)
        })
    };
    let uses = |block: &BlockSource| {
        let mut parents = Vec::new();
        let mut at = block.parent;
        while let Some(id) = at {
            let parent = &blocks[id.0 as usize];
            parents.push(parent.kind);
            at = parent.parent;
        }
        parents.reverse();
        let file = &chapters.get(block.range.0.chapter.0 as usize)?.path;
        let start = block.range.0.offset as usize;
        let text = text_of(file, start..start + block.range.1 as usize).unwrap_or_default();
        styles_of(block.kind, &parents, &text)
            .iter()
            .any(|used| used == style)
            .then_some(block.range.0)
    };
    blocks
        .iter()
        .filter(preferred)
        .find_map(uses)
        .or_else(|| blocks.iter().find_map(uses))
        .map(Target::Block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_is_set_in_the_style_its_kind_and_surroundings_give() {
        assert_eq!(styles_of(BlockKind::Heading(2), &[], "## A title"), ["heading-2"]);
        assert_eq!(styles_of(BlockKind::Paragraph, &[], "Plain text."), ["body"]);
        assert_eq!(
            styles_of(BlockKind::Paragraph, &[BlockKind::Quote], "> text"),
            ["quote"]
        );
        assert_eq!(
            styles_of(BlockKind::Paragraph, &[BlockKind::List, BlockKind::ListItem], "item"),
            ["list"]
        );
        assert_eq!(
            styles_of(
                BlockKind::Paragraph,
                &[BlockKind::Quote, BlockKind::List, BlockKind::ListItem],
                "x"
            ),
            ["list"]
        );
        assert_eq!(styles_of(BlockKind::Code, &[], "```\nx\n```"), ["code-block"]);
    }

    #[test]
    fn a_class_at_the_end_of_a_block_adds_its_custom_style() {
        assert_eq!(styles_of(BlockKind::Paragraph, &[], "Text. {.lead}"), ["body", "lead"]);
        assert_eq!(
            styles_of(BlockKind::Heading(1), &[], "# Part {.part-title}  \n"),
            ["heading-1", "part-title"]
        );
        assert_eq!(
            styles_of(BlockKind::Paragraph, &[], "Text with {braces} in it."),
            ["body"]
        );
    }

    #[test]
    fn directives_are_told_apart_by_their_name() {
        assert_eq!(
            styles_of(BlockKind::Directive, &[], "::: toc\n"),
            ["toc-heading", "toc-entry"]
        );
        assert_eq!(
            styles_of(BlockKind::Directive, &[], "::: page-break"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_specimen_has_a_block_for_every_style_the_theme_has() {
        use crate::preview::job::Typesetter;
        use crate::session::{Joined, Kind};
        use druck::cancel::Cancel;

        let specimen = include_str!("../../assets/specimen.md");
        let path = std::env::temp_dir().join("druck-specimen-probe.md");
        let Ok(layout) = Typesetter::new(Kind::Single { path: path.clone() }, None)
            .run(Joined::single(specimen.to_owned(), 0), &Cancel::default())
        else {
            panic!("the specimen typesets in the bundled theme")
        };
        let map = layout.laid.source_map().expect("the app asks for a source map");
        let text_of = |_: &Path, range: Range<usize>| specimen.get(range).map(str::to_owned);
        let missing: Vec<&str> = crate::inspector::style_keys()
            // A bibliography needs a `.bib` file beside the document, which a bundled text cannot bring.
            .filter(|style| *style != "bibliography")
            .filter(|style| find(map, style, None, &text_of).is_none())
            .collect();
        assert!(missing.is_empty(), "no block in the specimen for {missing:?}");
    }
}
