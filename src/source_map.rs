//! The connection between positioned text and the text as written, in both directions.
//!
//! A position is a byte offset in a chapter's file as written, comments and front matter included, so an editor's
//! buffer offsets and these agree without translation. A single file is chapter 0.
//!
//! The parser records on every piece of text which bytes of it are written as they stand, in [`Spans`]. Text that
//! smart punctuation, a no-break space, or a hyphen changed is marked as inserted, with the place it stands for.
//! Paragraph layout carries the spans through word and line breaking into each run. Text the engine generated for
//! a block, such as a heading's number, has no spans, only the block.
//!
//! The rule is that a mapping is exact or absent: [`SourceMap::position`] returns `None` for a byte that is not
//! written that way, and [`SourceMap::nearest`] says where to go for it instead. What maps to `None` is listed
//! there. Inside the engine, offsets count in the document's whole text, a project's joined chapters, until
//! [`SourceMap`] turns them into chapters.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::ops::Range;
use std::path::PathBuf;

use crate::config::source::InputSource;
use crate::document::{Block, Location};
use crate::output::{Item, Page, Run};

/// Marks a [`Piece`] whose bytes are not written as they stand.
const INSERTED: u32 = 1 << 31;

/// The largest document that can be mapped, since offsets leave one bit for [`INSERTED`].
pub(crate) const MAPPED_LENGTH: usize = INSERTED as usize;

/// Bytes of a text from `start` up to the next piece: written from a document offset onward, or if [`INSERTED`] is
/// set, inserted where that offset is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Piece {
    start: u32,
    source: u32,
}

impl Piece {
    fn offset(self) -> u32 {
        self.source & !INSERTED
    }

    fn written(self) -> bool {
        self.source & INSERTED == 0
    }
}

/// Where the bytes of a text come from, in pieces that cover it. Empty for generated text, and for all text when no
/// source map was asked for, so this costs nothing then.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Spans(Vec<Piece>);

impl Spans {
    /// Text written byte for byte, with `(byte in the text, document offset)` where each contiguous part starts.
    pub fn written(parts: impl IntoIterator<Item = (usize, usize)>) -> Self {
        let piece = |(start, source)| Piece {
            start: start as u32,
            source: source as u32,
        };
        Self(parts.into_iter().map(piece).collect())
    }

    /// Text that is not written as it stands and stands for the place `offset`.
    pub fn inserted(offset: usize) -> Self {
        Self(vec![Piece {
            start: 0,
            source: offset as u32 | INSERTED,
        }])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The spans of `range` of the text, as a text of its own.
    pub fn slice(&self, range: Range<usize>) -> Self {
        let (low, high) = (range.start as u32, range.end as u32);
        let mut sliced = Vec::new();
        for (index, piece) in self.0.iter().enumerate() {
            let end = self.0.get(index + 1).map_or(u32::MAX, |next| next.start);
            let from = piece.start.max(low);
            if from >= end.min(high) {
                continue;
            }
            let source = if piece.written() {
                piece.source + (from - piece.start)
            } else {
                piece.source
            };
            sliced.push(Piece {
                start: from - low,
                source,
            });
        }
        Self(sliced)
    }

    /// Keeps the first `length` bytes.
    pub fn truncate(&mut self, length: usize) {
        self.0.retain(|piece| (piece.start as usize) < length);
    }

    /// Adds `other`, whose text follows a text of `length` bytes.
    pub fn append(&mut self, other: Self, length: usize) {
        self.0.extend(other.0.into_iter().map(|piece| Piece {
            start: piece.start + length as u32,
            ..piece
        }));
    }

    /// The document offset after a text of `length` bytes, the place whatever follows it stands for.
    pub fn end(&self, length: usize) -> Option<usize> {
        let last = self.0.last()?;
        Some(match last.written() {
            true => last.source as usize + length - last.start as usize,
            false => last.offset() as usize,
        })
    }

    /// These spans after a change to their text, character by character. For each character of `from`, `changed`
    /// gives the length of what it became, and whether that still has the character's written place. Others become
    /// inserted text.
    fn rewritten(&self, from: &str, mut changed: impl FnMut(char) -> (usize, bool)) -> Self {
        if self.is_empty() {
            return Self::default();
        }
        let mut pieces: Vec<Piece> = Vec::new();
        let mut at = 0;
        for (byte, character) in from.char_indices() {
            let (length, same) = changed(character);
            let piece = self.piece_at(byte);
            let source = if piece.written() {
                let offset = piece.source + (byte as u32 - piece.start);
                if same { offset } else { offset | INSERTED }
            } else {
                piece.source
            };
            let joins = pieces.last().is_some_and(|last| {
                let run = at as u32 - last.start;
                if source & INSERTED == 0 && last.written() {
                    last.source + run == source
                } else {
                    last.source == source
                }
            });
            if !joins {
                pieces.push(Piece {
                    start: at as u32,
                    source,
                });
            }
            at += length;
        }
        Self(pieces)
    }

    /// The spans of `text` in capitals, for a style that sets it so. A character that changes length, as ß to SS,
    /// has no place of its own any more.
    pub fn uppercased(&self, text: &str) -> Self {
        if text.is_ascii() {
            return self.clone();
        }
        self.rewritten(text, |character| {
            let upper: String = character.to_uppercase().collect();
            let same = upper.len() == character.len_utf8() && upper.chars().count() == 1;
            (upper.len(), same)
        })
    }

    /// The spans of `text` with its tabs set as `width` spaces, as code is.
    pub fn detabbed(&self, text: &str, width: usize) -> Self {
        if !text.contains('\t') {
            return self.clone();
        }
        self.rewritten(text, |character| match character {
            '\t' => (width, false),
            other => (other.len_utf8(), true),
        })
    }

    /// The spans of `joined`, which is `text` with some of its spaces replaced by no-break spaces.
    pub fn joined(&self, text: &str, joined: &str) -> Self {
        if text.len() == joined.len() {
            return self.clone();
        }
        let mut rest = joined;
        self.rewritten(text, |character| {
            let inserted = character == ' ' && rest.starts_with('\u{a0}');
            let length = if inserted {
                '\u{a0}'.len_utf8()
            } else {
                character.len_utf8()
            };
            rest = &rest[length..];
            (length, !inserted)
        })
    }

    /// Adds a hyphen that a break inserted after a text of `length` bytes. It stands where the next byte is written.
    pub fn hyphenated(&mut self, length: usize) {
        let place = self.end(length).unwrap_or_default();
        self.0.push(Piece {
            start: length as u32,
            source: place as u32 | INSERTED,
        });
    }

    fn piece_at(&self, byte: usize) -> Piece {
        let index = self.0.partition_point(|piece| piece.start as usize <= byte);
        self.0[index.saturating_sub(1)]
    }
}

/// The number of a text in a [`SourceMap`]. Numbers follow the written order, so a later number is written later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextId(NonZeroU32);

/// Marks the number of a [`TextId`] as local to one line of layout, until the flow adopts it.
const LOCAL: u32 = 1 << 31;

impl TextId {
    /// A number for spans held by a line of layout, at `index` there.
    pub(crate) fn local(index: usize) -> Self {
        Self(NonZeroU32::new(index as u32 + 1 + LOCAL).expect("above zero"))
    }

    fn global(index: usize) -> Self {
        Self(NonZeroU32::new(index as u32 + 1).expect("a document has fewer texts than offsets"))
    }

    pub(crate) fn index(self) -> usize {
        (self.0.get() & !LOCAL) as usize - 1
    }

    pub(crate) fn is_local(self) -> bool {
        self.0.get() & LOCAL != 0
    }
}

/// A chapter of the document. A single file is one chapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chapter {
    pub path: PathBuf,
    /// Where it starts in the document's whole text.
    start: u32,
    /// What its file has before that start and the text lacks: a byte order mark.
    skip: u32,
}

/// The number of a chapter, in the order of the book. 0 for a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChapterId(pub u32);

/// A place in the written text: a byte offset in a chapter's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourcePos {
    pub chapter: ChapterId,
    pub offset: u32,
}

/// The number of a block in [`SourceMap::blocks`], in written order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

/// What a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    List,
    ListItem,
    Quote,
    Callout,
    Code,
    Image,
    Table,
    Footnote,
    SceneBreak,
    /// A `:::` directive, and the bibliography or index added where none is placed.
    Directive,
}

impl BlockKind {
    /// The kind as far as it tells blocks at one location apart.
    fn tag(self) -> u8 {
        match self {
            Self::Paragraph => 0,
            Self::Heading(_) => 1,
            Self::List => 2,
            Self::ListItem => 3,
            Self::Quote => 4,
            Self::Callout => 5,
            Self::Code => 6,
            Self::Image => 7,
            Self::Table => 8,
            Self::Footnote => 9,
            Self::SceneBreak => 10,
            Self::Directive => 11,
        }
    }
}

/// A block of the written text. Blocks are in written order, each nested one after its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSource {
    pub id: BlockId,
    pub kind: BlockKind,
    pub parent: Option<BlockId>,
    /// Where it starts and its length in bytes, within its chapter.
    pub range: (SourcePos, u32),
}

/// A piece of a run's text, for inspecting a mapping. See [`SourceMap::text`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The bytes of [`Run::text`].
    pub run: Range<u32>,
    /// Where these bytes are written, byte for byte. `None` for text the engine inserted.
    pub source: Option<SourcePos>,
}

/// What a text item is in the written text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSource {
    pub block: BlockId,
    /// Which bytes of the run come from where, covering it. Empty for text the engine generated for the block.
    pub spans: Vec<Span>,
}

/// A block as the parser finds it, with offsets in the document's whole text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBlock {
    kind: BlockKind,
    parent: Option<u32>,
    start: u32,
    end: u32,
}

type Key = (u64, u64, u8);

/// What the parser records about a document's blocks. Blocks carry only their location, so that finds them here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Sources {
    blocks: Vec<RawBlock>,
    keys: HashMap<Key, u32>,
    /// The spans of each code line, by the location of the code block.
    code: HashMap<(u64, u64), Vec<Spans>>,
}

impl Sources {
    /// Whether a block's lines are running text set by the paragraph composer, and not a heading, a table, or code.
    pub fn is_prose(&self, block: u32) -> bool {
        matches!(
            self.blocks.get(block as usize).map(|block| block.kind),
            Some(BlockKind::Paragraph | BlockKind::ListItem)
        )
    }

    /// The number of a block of the document. A block layout made has none.
    pub fn block_of(&self, block: &Block) -> Option<u32> {
        let kind = match block {
            Block::Paragraph { .. } => BlockKind::Paragraph,
            Block::Heading { level, .. } => BlockKind::Heading(*level),
            Block::List { .. } => BlockKind::List,
            Block::Quote { .. } => BlockKind::Quote,
            Block::Callout { .. } => BlockKind::Callout,
            Block::Code { .. } => BlockKind::Code,
            Block::Image { .. } => BlockKind::Image,
            Block::Table { .. } => BlockKind::Table,
            Block::SceneBreak { .. } => BlockKind::SceneBreak,
            Block::Keep { .. }
            | Block::Bottom { .. }
            | Block::Columns { .. }
            | Block::FullWidth { .. }
            | Block::PageBreak { .. }
            | Block::Matter { .. }
            | Block::Contents { .. }
            | Block::Bibliography { .. }
            | Block::Index { .. } => BlockKind::Directive,
        };
        self.block_at(block.at(), kind)
    }

    /// The number of the block of `kind` at `at`.
    pub fn block_at(&self, at: Location, kind: BlockKind) -> Option<u32> {
        self.keys.get(&(at.line, at.column, kind.tag())).copied()
    }

    /// The spans of line `index` of the code block at `at`.
    pub fn code_line(&self, at: Location, index: usize) -> Option<&Spans> {
        self.code.get(&(at.line, at.column))?.get(index)
    }
}

/// Records the blocks of a document while it is parsed.
pub(crate) struct Recorder {
    sources: Sources,
    /// The open blocks that can hold others, innermost last.
    stack: Vec<u32>,
    /// The paragraph or heading whose text is being read.
    leaf: Option<u32>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            sources: Sources::default(),
            stack: Vec::new(),
            leaf: None,
        }
    }

    fn add(&mut self, kind: BlockKind, at: Location, range: Range<usize>) -> u32 {
        let id = self.sources.blocks.len() as u32;
        self.sources.blocks.push(RawBlock {
            kind,
            parent: self.stack.last().copied(),
            start: range.start as u32,
            end: range.end as u32,
        });
        self.sources.keys.insert((at.line, at.column, kind.tag()), id);
        id
    }

    /// A block that holds others, until [`Recorder::close`].
    pub fn open(&mut self, kind: BlockKind, at: Location, range: Range<usize>) {
        let id = self.add(kind, at, range);
        self.stack.push(id);
    }

    /// Ends the innermost open block at `end`, if it is known only now.
    pub fn close(&mut self, end: Option<usize>) {
        if let (Some(id), Some(end)) = (self.stack.pop(), end) {
            self.sources.blocks[id as usize].end = end as u32;
        }
    }

    /// A block that holds no others.
    pub fn block(&mut self, kind: BlockKind, at: Location, range: Range<usize>) {
        self.add(kind, at, range);
    }

    /// A paragraph or heading, whose end follows its text.
    pub fn open_leaf(&mut self, kind: BlockKind, at: Location, start: usize) {
        self.leaf = Some(self.add(kind, at, start..start));
    }

    /// Extends the open paragraph or heading up to `end`.
    pub fn extend_leaf(&mut self, end: usize) {
        if let Some(leaf) = self.leaf {
            let block = &mut self.sources.blocks[leaf as usize];
            block.end = block.end.max(end as u32);
        }
    }

    pub fn close_leaf(&mut self) {
        self.leaf = None;
    }

    /// Makes the open paragraph an image at `at`.
    pub fn leaf_is_image(&mut self, at: Location) {
        if let Some(leaf) = self.leaf {
            self.sources.blocks[leaf as usize].kind = BlockKind::Image;
            self.sources
                .keys
                .insert((at.line, at.column, BlockKind::Image.tag()), leaf);
        }
    }

    /// Makes the directive just closed at `at` the table it holds.
    pub fn directive_is_table(&mut self, at: Location) {
        let key = (at.line, at.column, BlockKind::Directive.tag());
        if let Some(id) = self.sources.keys.get(&key).copied() {
            self.sources.blocks[id as usize].kind = BlockKind::Table;
            self.sources
                .keys
                .insert((at.line, at.column, BlockKind::Table.tag()), id);
        }
    }

    /// The spans of each line of the code block at `at`.
    pub fn code(&mut self, at: Location, lines: Vec<Spans>) {
        self.sources.code.insert((at.line, at.column), lines);
    }

    pub fn finish(self) -> Sources {
        self.sources
    }
}

/// The text items of a layout pass, in the order they are set.
#[derive(Default)]
pub(crate) struct Texts {
    entries: Vec<Text>,
    more: Vec<Piece>,
}

impl Texts {
    /// Adds a text of `block` with `spans`.
    pub fn add(&mut self, block: u32, spans: Spans) -> TextId {
        let entry = match spans.0.as_slice() {
            [] => Text::new(block, Head::Generated),
            [piece] if piece.start == 0 => Text::new(block, Head::One(piece.source)),
            pieces => {
                let first = self.more.len() as u32;
                self.more.extend_from_slice(pieces);
                Text::new(block, Head::Many(first))
            }
        };
        self.entries.push(entry);
        TextId::global(self.entries.len() - 1)
    }

    /// The block of a text, the document offset its first byte stands for, and whether its last byte is inserted.
    /// `None` for text the engine generated, which has no place of its own.
    pub fn place(&self, id: TextId) -> Option<(u32, u32, bool)> {
        let text = self.entries[id.index()];
        let (first, last) = match text.head() {
            Head::Generated => return None,
            Head::One(source) => (source, source),
            Head::Many(first) => {
                let pieces = &self.more[first as usize..];
                let count = 1 + pieces[1..].iter().take_while(|piece| piece.start != 0).count();
                (pieces[0].source, pieces[count - 1].source)
            }
        };
        Some((text.block(), first & !INSERTED, last & INSERTED != 0))
    }
}

/// A text item in the map: 8 bytes, since a document has one for each word. Most runs are one written piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Text {
    /// The block, with the kind of `head` in its top bits.
    block: u32,
    /// The source of the one piece, or the first piece in [`SourceMap::more`], whose start of 0 ends the one before.
    head: u32,
}

enum Head {
    One(u32),
    Many(u32),
    Generated,
}

const MANY: u32 = 1 << 31;
const GENERATED: u32 = 1 << 30;

impl Text {
    fn new(block: u32, head: Head) -> Self {
        let (tag, head) = match head {
            Head::One(source) => (0, source),
            Head::Many(first) => (MANY, first),
            Head::Generated => (GENERATED, 0),
        };
        Self {
            block: block | tag,
            head,
        }
    }

    fn block(self) -> u32 {
        self.block & !(MANY | GENERATED)
    }

    fn head(self) -> Head {
        if self.block & MANY != 0 {
            Head::Many(self.head)
        } else if self.block & GENERATED != 0 {
            Head::Generated
        } else {
            Head::One(self.head)
        }
    }
}

/// Connects the text items of a layout to the written text.
///
/// `position` and `nearest` go from a byte of a run to the written text, `locate` the other way.
///
/// Mapped to `None`, so never to a wrong place, are:
///
/// - smart punctuation: curly quotes, dashes, and ellipses made of straight ones;
/// - a no-break space that replaced a written space, and a hyphen a line break added;
/// - a capital that changed length, as ß to SS;
/// - a tab in code, which is set as spaces;
/// - the text of a `{todo: ...}`, whose white space is normalized;
/// - what the engine made: heading and list numbers, footnote markers, citations, cross-references, placeholders,
///   captions' labels, contents entries, and bibliography and index entries.
///
/// A footnote's text and a caption's are written text, and map to where they are written. Comments cut from the text
/// are skipped: bytes on both sides of one map to their own places. Running heads, page numbers, the title block,
/// and the watermark are outside every block and have no [`TextId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMap {
    chapters: Vec<Chapter>,
    blocks: Vec<BlockSource>,
    /// In written order. A text the engine made comes at the start of its block, before written text there.
    texts: Vec<Text>,
    /// The pieces of runs that have several, each run's starting at 0.
    more: Vec<Piece>,
    /// Where each text is first drawn, as page and item. A table header can repeat on later pages.
    places: Vec<(u32, u32)>,
}

impl SourceMap {
    /// Gives each text on `pages` its place and a number in written order. `source` names the chapters.
    pub(crate) fn build(
        Texts { entries, more }: Texts,
        sources: &Sources,
        pages: &mut [Page],
        source: &InputSource,
        document: &std::path::Path,
    ) -> Self {
        let chapters = chapters_of(source, document);
        let blocks: Vec<BlockSource> = (sources.blocks.iter().enumerate())
            .map(|(index, block)| {
                let start = position_in(&chapters, block.start);
                let chapter = &chapters[start.chapter.0 as usize];
                let chapter_end = chapters
                    .get(start.chapter.0 as usize + 1)
                    .map_or(u32::MAX, |next| next.start);
                BlockSource {
                    id: BlockId(index as u32),
                    kind: block.kind,
                    parent: block.parent.map(BlockId),
                    range: (start, block.end.min(chapter_end) - block.start.max(chapter.start)),
                }
            })
            .collect();

        // Where each text is first drawn. A text no page shows has no number.
        let mut places = vec![None; entries.len()];
        for (page_index, page) in pages.iter().enumerate() {
            for (item_index, item) in page.items.iter().enumerate() {
                if let Item::Text { source: Some(id), .. } = item
                    && !id.is_local()
                {
                    places[id.index()].get_or_insert((page_index as u32, item_index as u32));
                }
            }
        }
        let mut order: Vec<u32> = (0..entries.len() as u32)
            .filter(|index| places[*index as usize].is_some())
            .collect();
        // Text the engine made goes before written text at the same offset, so `locate` prefers the written text.
        let key = |text: Text| -> (u32, bool) {
            match text.head() {
                Head::One(source) => (source & !INSERTED, source & INSERTED == 0),
                Head::Many(first) => (more[first as usize].offset(), more[first as usize].written()),
                Head::Generated => (sources.blocks[text.block() as usize].start, false),
            }
        };
        order.sort_by_key(|index| key(entries[*index as usize]));
        let mut numbers = vec![u32::MAX; entries.len()];
        for (number, index) in order.iter().enumerate() {
            numbers[*index as usize] = number as u32;
        }
        for item in pages.iter_mut().flat_map(|page| &mut page.items) {
            if let Item::Text { source, .. } = item {
                // A local number is one the flow never adopted, whose spans are gone.
                *source = source
                    .filter(|id| !id.is_local())
                    .map(|id| TextId::global(numbers[id.index()] as usize));
            }
        }
        Self {
            chapters,
            blocks,
            places: order
                .iter()
                .map(|index| places[*index as usize].expect("only placed texts have numbers"))
                .collect(),
            texts: order.iter().map(|index| entries[*index as usize]).collect(),
            more,
        }
    }

    pub fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }

    pub fn blocks(&self) -> &[BlockSource] {
        &self.blocks
    }

    /// What a text is in the written text, for inspection. `run` is its run.
    pub fn text(&self, id: TextId, run: &Run) -> TextSource {
        let text = self.texts[id.index()];
        let length = run.text.len() as u32;
        let span = |start: u32, end: u32, piece: Piece| Span {
            run: start..end,
            source: piece.written().then(|| self.position_of(piece.source)),
        };
        let spans = match text.head() {
            Head::Generated => Vec::new(),
            Head::One(source) => vec![span(0, length, Piece { start: 0, source })],
            Head::Many(first) => {
                let pieces = self.pieces(first);
                (pieces.iter().enumerate())
                    .map(|(index, piece)| {
                        let end = pieces.get(index + 1).map_or(length, |next| next.start);
                        span(piece.start, end, *piece)
                    })
                    .collect()
            }
        };
        TextSource {
            block: BlockId(text.block()),
            spans,
        }
    }

    /// Where a byte of a run is written. `None` for a byte the engine inserted or generated, see [`SourceMap`].
    /// `byte` is at most the length of the run's text.
    pub fn position(&self, id: TextId, byte: usize) -> Option<SourcePos> {
        let piece = self.piece_at(self.texts[id.index()], byte)?;
        piece
            .written()
            .then(|| self.position_of(piece.source + (byte as u32 - piece.start)))
    }

    /// Where to go for a byte of a run: where it is written, the place inserted text stands for, or the start of
    /// the block for generated text.
    pub fn nearest(&self, id: TextId, byte: usize) -> SourcePos {
        let text = self.texts[id.index()];
        match self.piece_at(text, byte) {
            Some(piece) if piece.written() => self.position_of(piece.source + (byte as u32 - piece.start)),
            Some(piece) => self.position_of(piece.offset()),
            None => self.blocks[text.block() as usize].range.0,
        }
    }

    /// The first place where a written position is drawn, as page and item: the text that holds it, else the text
    /// before it, unless it is in the syntax that starts a block, which goes to that block's first text.
    pub fn locate(&self, at: SourcePos) -> Option<(usize, usize)> {
        let offset = self.document_offset(at)?;
        let after = self.texts.partition_point(|text| self.key(*text) <= offset);
        let next = (after < self.texts.len()).then_some(after);
        let before = after.checked_sub(1);
        let block = self.innermost(offset);
        let in_syntax = block.and_then(|block| {
            let start = self.document_offset(block.range.0)?;
            let next = next.filter(|next| self.key(self.texts[*next]) < start + block.range.1)?;
            let ahead = before.is_none_or(|before| self.key(self.texts[before]) < start);
            ahead.then_some(next)
        });
        let (page, item) = self.places[in_syntax.or(before).or(next)?];
        Some((page as usize, item as usize))
    }

    /// The byte of `id`'s run of `length` bytes that shows the written `at`, or the nearest.
    pub fn byte_of(&self, id: TextId, at: SourcePos, length: usize) -> usize {
        let Some(offset) = self.document_offset(at) else {
            return 0;
        };
        let text = self.texts[id.index()];
        let one;
        let pieces: &[Piece] = match text.head() {
            Head::Generated => return 0,
            Head::One(source) => {
                one = [Piece { start: 0, source }];
                &one
            }
            Head::Many(first) => self.pieces(first),
        };
        let mut byte = 0;
        for (index, piece) in pieces.iter().enumerate() {
            let end = pieces.get(index + 1).map_or(length as u32, |next| next.start);
            if piece.offset() > offset {
                break;
            }
            byte = piece.start;
            if piece.written() {
                byte += (offset - piece.offset()).min(end - piece.start);
            }
        }
        byte as usize
    }

    /// Bytes this map holds, beside what any layout holds, for a client that budgets memory.
    pub fn memory(&self) -> usize {
        use std::mem::size_of;
        size_of::<Self>()
            + self.chapters.capacity() * size_of::<Chapter>()
            + (self.chapters.iter())
                .map(|chapter| chapter.path.as_os_str().len())
                .sum::<usize>()
            + self.blocks.capacity() * size_of::<BlockSource>()
            + self.texts.capacity() * size_of::<Text>()
            + self.more.capacity() * size_of::<Piece>()
            + self.places.capacity() * size_of::<(u32, u32)>()
    }

    /// The pieces of a run with several.
    fn pieces(&self, first: u32) -> &[Piece] {
        let first = first as usize;
        let rest = &self.more[first + 1..];
        let count = rest.iter().position(|piece| piece.start == 0).unwrap_or(rest.len());
        &self.more[first..first + 1 + count]
    }

    fn piece_at(&self, text: Text, byte: usize) -> Option<Piece> {
        match text.head() {
            Head::Generated => None,
            Head::One(source) => Some(Piece { start: 0, source }),
            Head::Many(first) => {
                let pieces = self.pieces(first);
                let index = pieces.partition_point(|piece| piece.start as usize <= byte);
                Some(pieces[index.saturating_sub(1)])
            }
        }
    }

    /// Where a text stands in the written order.
    fn key(&self, text: Text) -> u32 {
        match text.head() {
            Head::One(source) => source & !INSERTED,
            Head::Many(first) => self.more[first as usize].offset(),
            Head::Generated => self
                .document_offset(self.blocks[text.block() as usize].range.0)
                .unwrap_or(0),
        }
    }

    fn position_of(&self, offset: u32) -> SourcePos {
        position_in(&self.chapters, offset & !INSERTED)
    }

    fn document_offset(&self, at: SourcePos) -> Option<u32> {
        let chapter = self.chapters.get(at.chapter.0 as usize)?;
        Some(chapter.start + at.offset.saturating_sub(chapter.skip))
    }

    /// The innermost block that holds a document offset. Blocks nest, so it is the last one that starts before it
    /// or one of the blocks around that one.
    fn innermost(&self, offset: u32) -> Option<&BlockSource> {
        let after = self
            .blocks
            .partition_point(|block| self.document_offset(block.range.0).is_some_and(|start| start <= offset));
        let mut block = self.blocks.get(after.checked_sub(1)?)?;
        loop {
            let start = self.document_offset(block.range.0)?;
            if offset < start + block.range.1 {
                return Some(block);
            }
            block = self.blocks.get(block.parent?.0 as usize)?;
        }
    }
}

pub(crate) fn position_in(chapters: &[Chapter], offset: u32) -> SourcePos {
    let index = chapters
        .partition_point(|chapter| chapter.start <= offset)
        .saturating_sub(1);
    let chapter = &chapters[index];
    SourcePos {
        chapter: ChapterId(index as u32),
        offset: offset - chapter.start + chapter.skip,
    }
}

pub(crate) fn chapters_of(source: &InputSource, document: &std::path::Path) -> Vec<Chapter> {
    if let InputSource::Chapters(map) = source {
        return (map.bytes())
            .map(|(path, start, skip)| Chapter {
                path: path.to_owned(),
                start: start as u32,
                skip: skip as u32,
            })
            .collect();
    }
    vec![Chapter {
        path: document.to_owned(),
        start: 0,
        skip: 0,
    }]
}
