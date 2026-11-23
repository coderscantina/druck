//! The audit of milestone 25: every construct the source layer knows has a way to it that needs no syntax. Each
//! `match` below names every variant, so a new kind of block, span, line marker, or directive does not compile until it
//! is given its path and its place in the walk here, and each path is checked against the menu, list, or command it
//! names.

use gpui::Action as _;

use super::commands::convert::{BlockType, Callout, TARGETS};
use super::commands::marks::Mark;
use super::commands::slash::{self, Action, Context, Standalone};
use super::directives::Kind;
use super::directives::edits::wraps_offered;
use super::directives::structure::Structure;
use super::float::MARK_BUTTONS;
use super::{
    CodeLanguage, InsertCitation, InsertFootnote, InsertReference, MarkIndex, OpenLink, SetStyle, ShowBlockMenu,
};
use crate::shell::strings::Language;
use crate::source::inline::{MarkerKind, SpanKind};
use crate::source::{BlockKind, Blocks};

/// How an author reaches a construct in Write.
enum Path {
    /// It is what typing makes: nothing to choose.
    Typing,
    /// An entry of the slash menu.
    Slash(Action),
    /// A type in Turn into, in the block menu.
    TurnInto(BlockType),
    /// A region in Wrap in, in the block menu.
    WrapIn(Kind),
    /// A button of the selection toolbar.
    Toolbar(Mark),
    /// A command of the palette, with its key, by its action's name.
    Command(&'static str),
    /// The book details panel, which writes a single file's front matter.
    Details,
    /// The link field writes it and keeps it out of sight.
    LinkField,
    /// The engine does not render it, so Write offers no way to make one.
    NotRendered,
    /// The Binder writes it from where a page stands in the book, so it never shows in the text.
    Binder,
}

fn block_paths(kind: BlockKind) -> Vec<Path> {
    match kind {
        BlockKind::FrontMatter => vec![Path::Details],
        BlockKind::Heading { .. } => vec![
            Path::Slash(Action::Turn(BlockType::Heading(1))),
            Path::TurnInto(BlockType::Heading(1)),
        ],
        BlockKind::Paragraph => vec![Path::Typing, Path::TurnInto(BlockType::Paragraph)],
        BlockKind::List => vec![
            Path::Slash(Action::Turn(BlockType::Bullets)),
            Path::Slash(Action::Turn(BlockType::Numbers)),
            Path::TurnInto(BlockType::Bullets),
        ],
        BlockKind::Quote => vec![
            Path::Slash(Action::Turn(BlockType::Quote)),
            Path::Slash(Action::Turn(BlockType::Callout(Callout::Note))),
            Path::TurnInto(BlockType::Quote),
        ],
        // The language is a list from the block menu, and Command+Return leaves the block.
        BlockKind::Code => vec![
            Path::Slash(Action::Turn(BlockType::Code)),
            Path::TurnInto(BlockType::Code),
            Path::Command(CodeLanguage.name()),
            Path::Command(super::LeaveCode.name()),
        ],
        BlockKind::Table => vec![Path::Slash(Action::Insert(Standalone::Table))],
        // Each directive has its own path, see `directive_paths`.
        BlockKind::Directive => walk(Kind::Columns, next_directive)
            .into_iter()
            .flat_map(directive_paths)
            .collect(),
        BlockKind::Comment => vec![Path::Slash(Action::Insert(Standalone::Comment))],
        BlockKind::SceneBreak => vec![Path::Slash(Action::Insert(Standalone::SceneBreak))],
        BlockKind::FootnoteDefinition => vec![Path::Slash(Action::OpenFootnote), Path::Command(InsertFootnote.name())],
        // Raw HTML is an error on the pages. A link definition is the link field's.
        BlockKind::Raw => vec![Path::NotRendered, Path::LinkField],
        BlockKind::Blank => vec![Path::Typing],
    }
}

fn directive_paths(kind: Kind) -> Vec<Path> {
    match kind {
        Kind::Columns => vec![Path::Slash(Action::Wrap("columns")), Path::WrapIn(Kind::Columns)],
        Kind::FullWidth => vec![Path::Slash(Action::Wrap("full-width"))],
        Kind::Keep => vec![Path::Slash(Action::Wrap("keep")), Path::WrapIn(Kind::Keep)],
        Kind::Bottom => vec![Path::Slash(Action::Wrap("bottom")), Path::WrapIn(Kind::Bottom)],
        // Its options, align, widths, and placement, are tools and fields of the table's grid.
        Kind::Table => vec![Path::Slash(Action::Insert(Standalone::ListTable))],
        Kind::PageBreak => vec![Path::Slash(Action::Insert(Standalone::PageBreak))],
        Kind::Toc => vec![Path::Slash(Action::Insert(Standalone::Toc))],
        Kind::Bibliography => vec![Path::Slash(Action::Insert(Standalone::Bibliography))],
        Kind::Index => vec![Path::Slash(Action::Section("index"))],
        Kind::FrontMatter | Kind::MainMatter | Kind::BackMatter => vec![Path::Binder],
    }
}

fn span_paths(kind: SpanKind) -> Vec<Path> {
    match kind {
        SpanKind::Emphasis => vec![Path::Toolbar(Mark::Italic)],
        SpanKind::Strong => vec![Path::Toolbar(Mark::Bold)],
        // Inline code has no button: a code block is the menu's way to code.
        SpanKind::Code => vec![Path::Slash(Action::Turn(BlockType::Code))],
        // A reference link opens in the same field, which writes its definition.
        SpanKind::Link => vec![Path::Command(OpenLink.name()), Path::LinkField],
        SpanKind::FootnoteReference => vec![Path::Slash(Action::OpenFootnote), Path::Command(InsertFootnote.name())],
        SpanKind::Citation => vec![Path::Slash(Action::OpenCitation), Path::Command(InsertCitation.name())],
        SpanKind::CrossReference => vec![
            Path::Slash(Action::OpenReference),
            Path::Command(InsertReference.name()),
        ],
        SpanKind::Comment => vec![Path::Slash(Action::Insert(Standalone::Comment))],
        SpanKind::Strike => vec![Path::Toolbar(Mark::Strike)],
        SpanKind::Superscript => vec![Path::Toolbar(Mark::Superscript)],
        SpanKind::Subscript => vec![Path::Toolbar(Mark::Subscript)],
        SpanKind::Underline => vec![Path::Toolbar(Mark::Underline)],
        SpanKind::SmallCaps => vec![Path::Toolbar(Mark::SmallCaps)],
        SpanKind::Todo => vec![Path::Slash(Action::Todo)],
        // Typing a brace and a letter opens the list of names, which Tab and Return take.
        SpanKind::Placeholder => vec![Path::Typing],
        SpanKind::Index => vec![Path::Command(MarkIndex.name())],
    }
}

fn marker_paths(kind: MarkerKind) -> Vec<Path> {
    match kind {
        MarkerKind::Heading => block_paths(BlockKind::Heading { level: 1 }),
        MarkerKind::Quote => block_paths(BlockKind::Quote),
        MarkerKind::Bullet => vec![Path::Slash(Action::Turn(BlockType::Bullets))],
        MarkerKind::Number => vec![Path::Slash(Action::Turn(BlockType::Numbers))],
    }
}

/// A custom style is no kind of block or span: it is `{.name}` on one. Its path is the list from the block menu, the
/// toolbar, and this command.
fn style_paths() -> Vec<Path> {
    vec![Path::Command(SetStyle.name()), Path::Command(ShowBlockMenu.name())]
}

/// Why `path` leads nowhere, if it does.
fn broken(path: &Path) -> Option<String> {
    let plain = "Text\n";
    match path {
        Path::Typing | Path::Details | Path::LinkField | Path::NotRendered | Path::Binder => None,
        Path::Slash(action) => {
            // Every entry is offered somewhere: in plain text of a book with works and labels, or for Full width inside
            // columns.
            let free = Context {
                has_works: true,
                has_labels: true,
                ..Context::default()
            };
            let in_columns = Context {
                open_directives: vec!["columns".to_owned()],
                ..Context::default()
            };
            let offered = [free, in_columns].iter().any(|context| {
                slash::items(context, Language::English)
                    .iter()
                    .any(|item| item.action == *action)
            });
            (!offered).then(|| format!("the slash menu has no entry for {action:?}"))
        }
        Path::TurnInto(target) => (!TARGETS.contains(target)).then(|| format!("Turn into lacks {target:?}")),
        Path::WrapIn(kind) => {
            let blocks = Blocks::parse(plain);
            let offered = wraps_offered(plain, &blocks, &Structure::default(), 0..1);
            (!offered.contains(kind)).then(|| format!("Wrap in lacks {kind:?}"))
        }
        Path::Toolbar(mark) => (!MARK_BUTTONS.iter().any(|(button, ..)| button == mark))
            .then(|| format!("the toolbar has no button for {mark:?}")),
        Path::Command(name) => {
            let listed = crate::shell::commands::all()
                .iter()
                .any(|command| command.action.name() == *name);
            (!listed).then(|| format!("the palette has no command {name}"))
        }
    }
}

/// Every value from `first` on, by `next`.
fn walk<T: Copy>(first: T, next: fn(T) -> Option<T>) -> Vec<T> {
    std::iter::successors(Some(first), |kind| next(*kind)).collect()
}

// The kind after each kind. A new kind has no place in the walk until it is given one, which its `match` demands.

fn next_block(kind: BlockKind) -> Option<BlockKind> {
    Some(match kind {
        BlockKind::FrontMatter => BlockKind::Heading { level: 1 },
        BlockKind::Heading { .. } => BlockKind::Paragraph,
        BlockKind::Paragraph => BlockKind::List,
        BlockKind::List => BlockKind::Quote,
        BlockKind::Quote => BlockKind::Code,
        BlockKind::Code => BlockKind::Table,
        BlockKind::Table => BlockKind::Directive,
        BlockKind::Directive => BlockKind::Comment,
        BlockKind::Comment => BlockKind::SceneBreak,
        BlockKind::SceneBreak => BlockKind::FootnoteDefinition,
        BlockKind::FootnoteDefinition => BlockKind::Raw,
        BlockKind::Raw => BlockKind::Blank,
        BlockKind::Blank => return None,
    })
}

fn next_directive(kind: Kind) -> Option<Kind> {
    Some(match kind {
        Kind::Columns => Kind::FullWidth,
        Kind::FullWidth => Kind::Keep,
        Kind::Keep => Kind::Bottom,
        Kind::Bottom => Kind::Table,
        Kind::Table => Kind::PageBreak,
        Kind::PageBreak => Kind::Toc,
        Kind::Toc => Kind::Bibliography,
        Kind::Bibliography => Kind::Index,
        Kind::Index => Kind::FrontMatter,
        Kind::FrontMatter => Kind::MainMatter,
        Kind::MainMatter => Kind::BackMatter,
        Kind::BackMatter => return None,
    })
}

fn next_span(kind: SpanKind) -> Option<SpanKind> {
    Some(match kind {
        SpanKind::Emphasis => SpanKind::Strong,
        SpanKind::Strong => SpanKind::Code,
        SpanKind::Code => SpanKind::Link,
        SpanKind::Link => SpanKind::FootnoteReference,
        SpanKind::FootnoteReference => SpanKind::Citation,
        SpanKind::Citation => SpanKind::CrossReference,
        SpanKind::CrossReference => SpanKind::Comment,
        SpanKind::Comment => SpanKind::Strike,
        SpanKind::Strike => SpanKind::Superscript,
        SpanKind::Superscript => SpanKind::Subscript,
        SpanKind::Subscript => SpanKind::Underline,
        SpanKind::Underline => SpanKind::SmallCaps,
        SpanKind::SmallCaps => SpanKind::Todo,
        SpanKind::Todo => SpanKind::Placeholder,
        SpanKind::Placeholder => SpanKind::Index,
        SpanKind::Index => return None,
    })
}

fn next_marker(kind: MarkerKind) -> Option<MarkerKind> {
    Some(match kind {
        MarkerKind::Heading => MarkerKind::Quote,
        MarkerKind::Quote => MarkerKind::Bullet,
        MarkerKind::Bullet => MarkerKind::Number,
        MarkerKind::Number => return None,
    })
}

#[test]
fn every_construct_the_source_layer_knows_has_a_way_to_it_without_syntax() {
    let blocks = walk(BlockKind::FrontMatter, next_block);
    let spans = walk(SpanKind::Emphasis, next_span);
    let markers = walk(MarkerKind::Heading, next_marker);
    assert_eq!((blocks.len(), spans.len(), markers.len()), (13, 16, 4));
    let constructs = (blocks.iter().map(|kind| (format!("{kind:?}"), block_paths(*kind))))
        .chain(spans.iter().map(|kind| (format!("{kind:?}"), span_paths(*kind))))
        .chain(markers.iter().map(|kind| (format!("{kind:?}"), marker_paths(*kind))))
        .chain(std::iter::once(("custom style".to_owned(), style_paths())));
    let mut problems = Vec::new();
    for (name, paths) in constructs {
        if paths.is_empty() {
            problems.push(format!("{name} has no path"));
        }
        problems.extend(paths.iter().filter_map(broken).map(|why| format!("{name}: {why}")));
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
