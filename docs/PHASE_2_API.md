# Phase 2 API design

The concrete design for [the phase 2 contract](RUST_CRATE_API_PHASE_2.md), written on 2026-11-03 against the engine after milestone 15 and the [desktop spike results](DECISIONS.md#2026-11-03-desktop-spike-results). It decides crate layout, entry paths, resource resolution, renderer ownership, the layout result, source mapping, diagnostics, cancellation, versioning, and the migration order. Signatures are sketches: names are settled, bodies and derives are not.

Two clients drive every choice: the desktop app, which lays out the same document again on each pause with one set of fonts, and a batch render in a CLI or a service, which lays out once and writes bytes. Nothing here serves a third.

## 1. Crate layout

One package, `druck`, with a library target and a binary target. The library is the public crate. The binary is the CLI and uses only the library's public surface, which the compiler enforces: a binary target is a separate crate and cannot see `pub(crate)` items of the library. The desktop stays the workspace member `druck-desktop` and depends on `druck` by path.

```toml
[package]
name = "druck"            # unchanged: cargo install druck, the release tag, build.rs

[features]
default = ["cli"]
cli = ["dep:clap"]        # the binary's own dependencies
editor = []               # unstable module for the desktop, see below

[[bin]]
name = "druck"
required-features = ["cli"]
```

Why one package: `cargo install druck`, `scripts/release.sh` (which rewrites the first `version =` in `Cargo.toml` and runs `cargo update --workspace`), `scripts/notices.py` (which walks from the root package), `build.rs`, and the release workflow all keep working without edits. A second crate would need a workspace version, two publishes, and a changed notices walk for nothing the clients need. The cost is that a library user who does not disable default features compiles clap. That is acceptable and documented.

`src/lib.rs` becomes a facade. Every `pub mod` it lists today becomes `mod`, and the root re-exports the public types from a few new modules:

```rust
// src/lib.rs
mod bibliography; mod citations; mod config; mod date; mod document; mod epub; mod highlight;
mod image; mod index; mod installed; mod layout; mod markdown; mod page; mod pdf; mod pipeline;
mod project; mod spacing; mod statistics; mod tags; mod text; mod theme_path;

pub mod diagnostic;   // Diagnostic, Severity, Source, Location
pub mod resource;     // Resolver, FileResolver, MemoryResolver, Origin, ResourceId, Bytes
pub mod input;        // Input, ThemeSelection, Overrides, Project, Export
pub mod output;       // Layout, Page, Item, Run, Glyph, Face, Picture, Link, SourceMap types
pub mod cancel;       // Cancel
mod renderer;         // Renderer, Job, Configured, Rendered, PdfOptions, EpubOptions, Failure

pub use renderer::*;
pub const API_VERSION: u32 = 1;
pub const THEME_SCHEMA_VERSIONS: &[u64] = &[1];

#[cfg(feature = "editor")]
#[doc(hidden)]
pub mod editor;       // unstable: citation syntax scanner, inline marks, theme value merge
```

What stays private, as the contract asks: the parser (`markdown`, `document`), citations and bibliography formatting, `layout` and its scoring, `tags`, `page` (replaced by `output`), `pipeline`, `config::resolve` and `config::theme` (the resolved `Config` is opaque, see section 2), `text` (replaced by `output::Face`), `installed`, `theme_path`, and `highlight`.

The desktop today reaches `bibliography::syntax`, `markdown::marks`, `config::merge`, `config::layer::default_theme_value`, `theme_path`, and the fields of `Config`. The contract excludes editor components, and these are parser internals. They move behind the `editor` feature as `druck::editor::{syntax, marks, theme_value, resolved_config}`, marked unstable and exempt from `API_VERSION`. The desktop is in the workspace and pins the path dependency, so it can follow changes. This is the first decision worth the owner's review: the alternative, making the citation scanner public, puts parser syntax under a compatibility promise for one client.

## 2. Entry paths

```rust
pub struct Renderer { /* caches, section 4 */ }

impl Renderer {
    pub fn new() -> Self;
    /// The simple path: Markdown and configuration in, PDF bytes and diagnostics out.
    pub fn render(&self, resolver: &dyn Resolver, input: &Input, options: &PdfOptions, cancel: &Cancel)
        -> Result<Rendered, Failure>;
    /// The explicit path. A job is one document on one thread.
    pub fn job<'r>(&'r self, resolver: &'r dyn Resolver, input: &'r Input, cancel: &'r Cancel) -> Job<'r>;
}

pub struct Job<'r> { /* renderer, resolver, input, cancel */ }

impl Job<'_> {
    /// Resolves and checks the configuration. Errors here never reach layout.
    pub fn configure(self) -> Result<Configured<'_>, Failure>;
}

pub struct Configured<'r> { /* Config, the prepared document, and the job */ }

impl Configured<'_> {
    pub fn warnings(&self) -> &[Diagnostic];
    pub fn config_json(&self) -> serde_json::Value;          // what `druck check --print-config` prints
    pub fn todos(&self) -> &[Todo];                          // open placeholders, for final exports
    pub fn layout(&self, options: &LayoutOptions) -> Result<Layout, Failure>;
    pub fn epub(&self, options: &EpubOptions) -> Result<Rendered, Failure>;   // needs no layout
}

impl Layout {
    /// Writes the PDF from the display list alone. Nothing is shaped or paginated again.
    pub fn pdf(&self, options: &PdfOptions) -> Result<Rendered, Failure>;
}

pub struct Rendered { pub bytes: Vec<u8>, pub warnings: Vec<Diagnostic> }
pub struct LayoutOptions { pub source_map: bool }                 // default false
pub struct PdfOptions { pub bleed: Pt, pub crop_marks: bool }     // export only, never changes layout
pub struct EpubOptions {}
pub enum Failure { Cancelled, Errors(Vec<Diagnostic>) }
```

`render` is `job().configure()?.layout(&LayoutOptions::default())?.pdf(options)` and nothing else, so both paths resolve identical inputs identically by construction. Today's `pdf::write` shapes one space glyph per run to find the space glyph id; that becomes a cmap lookup on the prepared face, so export needs no shaper.

Inputs are values, with origins instead of paths, so a service can render from memory:

```rust
pub struct Input {
    pub text: DocumentText,
    pub theme: ThemeSelection,
    pub overrides: Overrides,
    pub today: Date,                        // the caller reads the clock and SOURCE_DATE_EPOCH
}

pub enum DocumentText {
    /// One Markdown file with front matter. `origin` is the folder its relative paths start from.
    Single { text: String, origin: Origin, name: String },
    /// A project: joined chapters without front matter, settings from the manifest.
    Project(Project),
}

pub enum ThemeSelection {
    /// Nothing asked for: the document's `theme` setting, resolved against `dirs`.
    Document { dirs: ThemeDirs },
    Name { name: String, dirs: ThemeDirs },
    File { path: PathBuf },
    Json { text: String, origin: Origin },  // in memory, assets relative to the origin
}

/// Where a bare theme name is looked up, in order. No environment or home directory is read.
pub struct ThemeDirs { pub project: Option<PathBuf>, pub shared: Option<PathBuf> }

pub struct Overrides(/* FrontMatter parsed from KEY=VALUE pairs or JSON */);
impl Overrides {
    pub fn parse(pairs: &[String]) -> Result<Self, Vec<Diagnostic>>;   // --set syntax
    pub fn from_json(value: serde_json::Value) -> Result<Self, Vec<Diagnostic>>;
}
```

The typed configuration the contract asks for is these inputs plus validation. The resolved `Config` stays opaque and is inspected as JSON. Exposing its forty fields and every theme type would put the whole schema under the API version, and neither client reads them typed except the desktop inspector, which gets them through the `editor` feature.

Project folders are in the crate, as `input::Project`, because the CLI, the app, and a service that receives a folder all need the same manifest rules, and the rules are layout-relevant (book-wide labels, chapter locations in diagnostics). Reading the manifest and chapters goes through the resolver, with `Origin::Document(dir)`, and listing `manuscript/` for unlisted files through `Resolver::list`. What stays above the crate: choosing the output path (`build/<name>.pdf`), creating folders, and writing files.

```rust
impl Project {
    pub fn open(resolver: &dyn Resolver, dir: &Path) -> Result<Self, Vec<Diagnostic>>;
    pub fn warnings(&self) -> &[Diagnostic];                // chapters not in the book
    pub fn exports(&self) -> &BTreeMap<String, Export>;
    /// The export's theme and draft applied, and its print and final settings returned beside the text.
    pub fn export(self, name: Option<&str>) -> Result<(DocumentText, ExportSettings), Vec<Diagnostic>>;
}
pub struct ExportSettings { pub pdf: PdfOptions, pub is_final: bool, pub output: Option<PathBuf> }
```

## 3. Resource resolver

```rust
pub trait Resolver: Send + Sync {
    /// The bytes of `path` relative to `origin`. Errors are plain messages; the engine adds the
    /// resource, origin, property, and source location.
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError>;
    /// Relative paths of the files below `dir`, for a project's `manuscript/`. `None` where listing
    /// is not supported, which the engine treats as an empty folder.
    fn list(&self, origin: &Origin, dir: &str) -> Option<Vec<String>> { None }
    /// Files of an installed family, for themes that name fonts without files. The default finds none.
    fn installed(&self, family: &str) -> Result<Vec<InstalledFace>, ResolveError> { Ok(Vec::new()) }
}

/// Immutable bytes with a content hash computed once. `Arc<[u8]>` inside.
#[derive(Clone)]
pub struct Bytes { data: Arc<[u8]>, hash: u64 }

pub enum ResolveError { NotFound, Unreadable(String), Unsupported }

pub struct FileResolver { /* no state; installed fonts scanned lazily once */ }
pub struct MemoryResolver { files: HashMap<(Origin, String), Bytes> }
impl MemoryResolver { pub fn insert(&mut self, origin: Origin, path: &str, data: impl Into<Arc<[u8]>>); }
```

`Origin` stays as today (`Bundled`, `Theme(dir)`, `Document(dir)`, `WorkingDir(dir)`, `Installed`), and `Resource { origin, path }` keeps its `Display` for messages. Bundled fonts and the default theme never reach the resolver: `Origin::Bundled` is answered from `text::bundled` inside the engine, so a `MemoryResolver` with nothing in it renders a plain document.

Installed font lookup moves into `FileResolver::installed`, which wraps today's `installed::system` and `installed::lookup`. The core then performs no discovery, the memory resolver reports the family as not found with the property that asked for it, and the desktop can offer its own cache later.

`check_resources` goes away. It exists so that a missing file fails before layout, but with a resolver every resource is read during `configure` (fonts, theme images, the bibliography, the cover) or `prepare` (document images), and each read failure is a diagnostic with the resource, origin, property, and, for document images, the line. Reading at configure time gives the same early failure without the stat.

The process-wide display path (`show_relative_to`, `shown`) goes away. `Source` and `Resource` display absolute paths. The CLI shortens them with `Diagnostic::display_from(&self, base: &Path)`, section 7.

Changed contents are detected by content, not by path: every job reads every resource it uses through the resolver and keys reuse by `Bytes::hash` (a 64-bit hash over the bytes, computed when `Bytes` is made). The file resolver reads a font file again on each job, which the spike did not, and that is the fix for the font replaced under the same path. Cost: a theme names a few megabytes of font files, which is a few milliseconds to read from the page cache and hash, against 390 ms of layout; step 2 measures it on the corpus. If that shows in a measurement, `FileResolver` can later keep `(len, mtime)` to skip the read, but only inside the resolver, where the contract's reuse rule is a local decision. The memory resolver hashes once at `insert`.

## 4. Reusable renderer and font ownership

Verified against the installed sources: `rustybuzz::Face<'a>` (0.20.1, `hb/face.rs`) borrows its bytes and has no owned form; `rustybuzz::ShapePlan` (`hb_ot_shape_plan_t`) has no lifetime and holds `Box<dyn Any + Send + Sync>`, so it can outlive the face it was built from and be shared; `krilla::text::Font` (0.8.2) is `Font(Arc<..>)`, built from `krilla::Data`, which is `Arc<dyn AsRef<[u8]> + Send + Sync>` with `From<Arc<Vec<u8>>>`. So owned bytes work for krilla directly, and for rustybuzz by parsing a borrowed `Face` per job.

```rust
/// What the renderer keeps per face across jobs. Immutable once built.
pub struct Face {
    data: Bytes,                                   // the font file
    index: u32,
    coordinates: Vec<([u8; 4], f32)>,              // the instance, in axis units
    pdf: krilla::text::Font,                       // owned, shares `data`
    file: String,                                  // for diagnostics
    restricted: bool,
    plans: RwLock<HashMap<PlanKey, Arc<rustybuzz::ShapePlan>>>,
}
impl Face {
    pub fn data(&self) -> &[u8]; pub fn index(&self) -> u32; pub fn coordinates(&self) -> &[([u8; 4], f32)];
}

/// Keyed by what decides a face: the bytes, the face index, and the variation instance.
type FaceKey = (u64 /* Bytes::hash */, u32, Vec<([u8; 4], f32)>);

pub struct Renderer {
    faces: RwLock<HashMap<FaceKey, Arc<Face>>>,
    images: RwLock<HashMap<u64 /* Bytes::hash */, Picture>>,   // decoded, see section 5
    budget: usize,                                             // bytes of decoded images kept
}
```

A job builds the private `text::Fonts<'j>` from the `Arc<Face>`s the configuration names: `rustybuzz::Face::from_slice(&face.data, face.index)` with `set_variations`. That parse is a table directory read and GSUB/GPOS wrapper construction, microseconds per face, done once per job for a handful of faces. The shape plan cache moves from `Fonts` into `Face`, so plans survive across jobs and the spike's 0.67 s to 0.39 s gain is kept without the leak. A paragraph cache, when it comes, lives in `Renderer` beside `images`, keyed by the configuration hash, the face keys, and the paragraph's inline content; the interface already puts every cross-job state in `Renderer`, so adding it changes no signature.

`Vec::leak` and the `FILES` static in `text::read` go. Memory is bounded by what the renderer holds: faces are kept while any job or layout references them and the renderer's table entry is dropped when a job ends and the face was not used by it or the previous job. Decoded images are kept up to `budget` bytes (default 256 MB) with the same two-job rule, which keeps the desktop's refresh from decoding again and lets a batch render drop them at the end.

Thread safety, enforced by types rather than documentation:

- `Renderer: Send + Sync`. Independent jobs run from `&Renderer` on caller threads, `std::thread::scope` or a pool; the crate owns no threads.
- `Job` and `Configured` borrow the renderer, resolver, input, and cancel token for a lifetime and are `Send` but not `Sync`: one job, one thread.
- `Layout: Send + Sync`, owned, and cheap to put in an `Arc`. It holds `Arc<Face>` and `Picture` clones, so it outlives the input, the resolver, and the renderer.
- `Cancel: Clone + Send + Sync`.
- `dyn Resolver` must be `Send + Sync`, which the trait requires.

A test asserts these bounds with `fn assert<T: Send + Sync>()`.

## 5. Layout result

`page::Output` is already a self-contained display list except that runs name fonts by `FaceId` into a `Fonts` that leaks. The public `Layout` replaces `FaceId` with an index into its own face table and keeps the rest:

```rust
pub struct Layout {
    pages: Vec<Page>,
    faces: Vec<Arc<Face>>,            // indexed by Run::face
    anchors: Vec<Position>,
    outline: Vec<Bookmark>,
    page_numbers: Vec<Option<PageNumber>>,
    tags: tags::Tree,                 // private: PDF export reads it, nothing else
    metadata: Metadata, lang: Lang, conformance: Conformance,   // private, for export
    warnings: Vec<Diagnostic>,
    source: Option<SourceMap>,        // section 6
}
impl Layout {
    pub fn pages(&self) -> &[Page];
    pub fn face(&self, id: FaceId) -> &Face;
    pub fn anchor(&self, id: AnchorId) -> Position;
    pub fn outline(&self) -> &[Bookmark];
    pub fn page_numbers(&self) -> &[Option<PageNumber>];
    pub fn warnings(&self) -> &[Diagnostic];
    pub fn source_map(&self) -> Option<&SourceMap>;
}

pub struct Page { pub width: Pt, pub height: Pt, pub items: Vec<Item> }

pub enum Item {
    Text { x: Pt, y: Pt, run: Run, color: Color, source: Option<TextSource> },
    TurnedText { x: Pt, y: Pt, angle: f64, run: Run, color: Color },
    Rect { rect: Rect, color: Color },
    Link { rect: Rect, link: Link },
    Image { rect: Rect, picture: Picture },
    Anchor { id: AnchorId, x: Pt, y: Pt },
    /// Structure boundaries. Not drawn; a painter skips them.
    Begin(Structure), End,
}

pub struct Run { pub text: String, pub face: FaceId, pub size: Pt, pub glyphs: Vec<Glyph>, pub width: Pt }
pub struct Glyph { pub id: u16, pub x_advance: Pt, pub x_offset: Pt, pub y_offset: Pt, pub text: Range<usize> }

/// A decoded image. Cheap to clone; the PDF writer and a painter read the same decoded data.
#[derive(Clone)]
pub struct Picture { /* today's image::Image */ }
impl Picture {
    pub fn size(&self) -> (Pt, Pt);
    pub fn source(&self) -> &[u8];                   // the file as read, for painters that decode themselves
    pub fn pixels(&self) -> Pixels;                  // Raster { width, height, rgba: &[u8] } | Svg(&usvg::Tree)
}

pub enum Link { Url(String), Anchor(AnchorId) }
pub enum Structure { Element(ElementKind), Link, Note, Artifact }   // the public face of tags::Mark
```

What of today's types is public: `page::Item` as above with `Begin(Mark)` reduced to a small `Structure` enum (the desktop painter skips these, verified in `desktop/src/preview/raster.rs`, so clients need only to know they are not drawn). `ShapedRun` becomes `Run` without `spacing`, which only line breaking reads. `tags::Tree`, `Id`, `Kind`, and `Node` stay private: PDF export is the only consumer. `Aside` stays private; only tests read it. `NoteKind` goes with it.

The desktop painter reads `Item::{Text, TurnedText, Rect, Image}`, `run.glyphs`, `run.face`, `run.size`, `Fonts::outlines` (data, index, coordinates), `Image::pixels`, and `Image::source`, and fingerprints a page from the same fields. Every one of these is on `Layout`, `Face`, or `Picture`, so the painter works from a `Layout` alone and keeps an `Arc<Layout>` per published result instead of an `Arc<Painter>` wrapping `Fonts`.

Documented coordinates, as `page.rs` states today: points, origin at the top left of the page, y growing down. `Page::width` and `height` are the trim size; bleed and crop marks exist only in `PdfOptions`. Items draw in order, later over earlier. A run draws from `(x, y)` on the baseline: for each glyph the outline of glyph `id` in `face.data()` at `face.index()` with `face.coordinates()`, scaled by `size / units_per_em`, at `(pen + x_offset, y - y_offset)`, then `pen += x_advance`. `TurnedText` turns the run `angle` degrees counterclockwise about `(x, y)`. `Glyph::text` is the byte range of `Run::text` the glyph covers, empty for the second and later glyphs of a cluster. An image fills `rect`. `Link` rectangles and `Anchor` points are not drawn.

PDF export from a `Layout` is today's `pdf::write` with `Fonts` replaced by `Layout::faces` for `pdf_font` and `file`, and the space glyph found through `ttf_parser::Face::glyph_index(' ')` on the prepared face. It paginates nothing, shapes nothing, and resolves nothing.

## 6. Source mapping

The galley and the pages must locate each other both ways, and diagnostics already name chapter files. The engine knows byte offsets: the parser works from `pulldown-cmark`'s offset iterator, `Builder::location` turns an offset in the comment-cut text into a written-text location through `Cuts::original`, and `Project::join` records where each chapter starts. None of this reaches layout today, because `Location` is line and column, and runs carry only their own text.

Positions are byte offsets in a chapter's text as written, including comments and front matter, so the app's buffer offsets and the engine's agree without translation:

```rust
pub struct SourcePos { pub chapter: ChapterId, pub offset: u32 }
pub struct ChapterId(pub u16);     // 0 for a single file

pub struct SourceMap {
    chapters: Vec<Chapter>,                 // path, for diagnostics and the app's buffer lookup
    blocks: Vec<BlockSource>,               // document order; nested blocks follow their parent
    block_items: Vec<Vec<(u32, u32)>>,      // per block: (page, item) of its text items, reading order
}
pub struct Chapter { pub path: Option<PathBuf> }
pub struct BlockSource { pub id: BlockId, pub kind: BlockKind, pub parent: Option<BlockId>, pub range: (SourcePos, u32) }
pub struct BlockId(pub u32);
pub enum BlockKind { Paragraph, Heading(u8), List, ListItem, Quote, Callout, Code, Image, Table, Footnote, Directive }

/// On every `Item::Text` of a mapped layout.
pub struct TextSource {
    pub block: BlockId,
    /// Which bytes of `Run::text` come from where. Empty for text the engine generated for the block:
    /// heading numbers, footnote markers, contents entries, running heads, bibliography entries.
    pub spans: Arc<[Span]>,
}
/// `run` bytes come from `source` onward, byte for byte. `source` is `None` for inserted text: a
/// hyphen added at a break, a no-break space that replaced a written space, smart punctuation.
pub struct Span { pub run: Range<u32>, pub source: Option<u32> }

impl SourceMap {
    /// The text item nearest to a source position, for a click in the galley.
    pub fn locate(&self, layout: &Layout, at: SourcePos) -> Option<(usize /* page */, usize /* item */)>;
    /// The source position of a byte of a run, for a click on a page.
    pub fn position(&self, text: &TextSource, run_byte: usize) -> Option<SourcePos>;
}
```

Encoding: the parser records on every `Inline::Text` the comment-cut range it came from and whether the text is verbatim; `spacing::join` splits a verbatim piece at each no-break space it inserts; paragraph layout carries the pieces through `tokens`, `Fragment`, and `line_runs` into each run's spans, marking the `-` appended for a hyphenated break and the small capitals the layout builds by reshaping; `Cuts::original` and the chapter table turn cut offsets into `SourcePos` at the end. Generated text gets the block it belongs to and no spans, so a click on a contents entry still goes to its heading. Decoration without a block, such as page numbers and the watermark, has no `TextSource`.

Cost for the corpus (242,826 bytes, 45,404 words, 203 pages): a run is a line fragment, so roughly 25,000 runs; most have one span, hyphenated and joined ones two or three, so about 35,000 spans at 16 bytes, plus 8 bytes of block id and `Arc` pointer per run, near 0.8 MB. The block table is a few thousand entries at 24 bytes, under 100 KB. Today's `Glyph` alone is 48 bytes times roughly 240,000 glyphs, about 11.5 MB, so the map adds under ten percent and no per-glyph table. It is still opt-in through `LayoutOptions::source_map`, because a batch render builds it for nobody, and the parser's range bookkeeping is skipped when off. Step 4 measures the real number.

Both directions: galley to pages is `locate`, which finds the innermost block whose range holds the position (the last block in document order that contains it), then the item of that block whose span starts at or before the offset. Pages to galley is a hit test the painter does on its own items, then `position` on the hit run's `TextSource`. A click on generated text yields the block's start.

## 7. Diagnostics

```rust
pub struct Diagnostic {
    pub severity: Severity,                 // Error | Warning
    pub message: String,
    pub source: Option<Source>,
    pub location: Option<Location>,         // 1-based line and column in `source`
    pub property: Option<String>,           // dotted theme or setting path, "styles.body.size"
    position: (u32, u32),                   // private: reading order across chapters
}
pub enum Severity { Error, Warning }
pub enum Source { Bundled, Theme(PathBuf), Document(PathBuf), Bibliography(PathBuf), Overrides }
impl Diagnostic {
    /// For terminals: paths under `base` shown relative to it, else absolute.
    pub fn display_from<'a>(&'a self, base: &'a Path) -> impl Display + 'a;
}
```

Public categories, with stable meaning: `Severity` and `Source` as above, a location with a chapter file where one applies (the project join already rewrites `Source::Chapters` into the chapter's `Source::Document` in `Diagnostic::at`, so the `Chapters` variant becomes private), and the property path for theme and setting errors. Codes are not added. `Failure::Errors` carries every error of the stage that failed, sorted in reading order, as today.

Warnings beside success: `Configured::warnings` (configuration), `Project::warnings` (chapters not in the book), `Layout::warnings` (typesetting rules broken), and `Rendered::warnings` (embedding restrictions, EPUB warnings, and the layout's warnings again when `render` was used). The engine never prints. `pipeline::load` and `Job::open_project` in `main.rs` print configuration and project warnings to stderr today, and `typeset`, `render`, and `build_epub` print layout, embedding, and EPUB warnings; all of that moves into the CLI, which prints each list where it printed before, so terminal output does not change.

The CLI prints exactly what it prints today: `error:` and `warning:` lines through `display_from(working_dir)`, the `wrote N pages` and `document is valid` lines, and the broken-rules count after a render.

## 8. Cancellation

```rust
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel {
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
    pub(crate) fn check(&self) -> Result<(), Failure>;   // Err(Failure::Cancelled) once set
}
```

`check` is called at these points, which are the loops that take time:

- `pipeline::prepare`, before each image decode in `load_images` and `load_theme_images`, and before the bibliography parse.
- `text::Fonts` construction, before parsing each face.
- `layout::settle`, before each pass.
- `layout::Flow::blocks`, at the top of `for block in blocks` (`layout/mod.rs:752`), which also covers footnotes and table cells since they run through the same function.
- `layout::pages::compose`, in the pagination search `for at in 0..=count` (`layout/pages.rs:819`), every 64 lines.
- `pdf::write`, before each page.
- `epub::write`, before each chapter document.

Spacing, estimated: the corpus lays out in 390 ms over roughly 1,500 blocks and 7,000 lines, so a block takes about 0.25 ms on average and the longest single block, a two-page paragraph with its line breaking, around 20 ms. Pagination checks every 64 lines at well under a millisecond each. Image decoding is the one long step: a 20 MB photograph takes a few hundred milliseconds in `Image::decode`, and the check before it is the latest point. So a superseded preview stops within about 20 ms in layout, within one page in PDF export, and in the worst case within one image decode, which the image cache makes rare. A cancelled job returns `Failure::Cancelled` from the stage it was in, never partial bytes or a partial layout, and leaves the renderer's caches as they were because they only ever gain completed faces and images.

The desktop's `Scheduler`, which waits for a running layout because "a layout cannot be stopped", becomes: cancel the running job's token, start the newest text at once. The 150 ms pause stays.

## 9. Compatibility and versioning

The package version stays CalVer (`26.10.11`), which Cargo reads as semver with a changing major, so no compatibility is implied by the number. The API promise is made explicitly: `druck::API_VERSION` is a counter bumped on any breaking change to the public modules of section 1, with a changelog section naming what broke; a release that does not bump it keeps every public signature. The desktop pins the path dependency and can follow. If an outside client appears, the choice between keeping this and splitting a semver library crate is revisited then, which is the second decision worth review.

The theme schema is versioned by the `version` field, checked in `config::layer` against `THEME_VERSION`. `druck::THEME_SCHEMA_VERSIONS` lists what the release accepts, and an unsupported version fails in `configure` with the existing property diagnostic. `druck.yaml` has no version, as decided on 2026-11-03.

Private and free to change: everything not re-exported in section 1, the hash algorithm of `Bytes`, the cache eviction rule, the shape plan key, the layout scoring, the tag tree, and the `editor` module.

## 10. Migration order

The guard for every step: a release build renders every sample and the ttp404 corpus before and after, and `cmp` shows byte-identical PDFs and EPUBs (`scripts/render-samples.sh` and the corpus `--export print`). The samples have ISO dates, so the bytes are stable. Each step also passes `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.

1. **Fence the surface.** Owns `Cargo.toml`, `src/lib.rs`, `src/main.rs`, new `src/renderer.rs` as a thin wrapper over `pipeline`, and the `cli` and `editor` features. The desktop switches its imports to `druck::editor` for internals and keeps using the wrapper for the rest. Acceptance: `cargo doc --no-deps` lists only the section 1 modules; `cargo install --path . --locked` installs a working binary; `cargo build -p druck-desktop` builds; the byte guard holds. Runs first, alone, because every later step builds on the facade.
2. **Resolver and owned fonts.** Owns `src/resource.rs`, `src/text/`, `src/image.rs`, `src/installed.rs`, `src/pipeline.rs`, `src/config/source.rs`, and `src/project.rs`. Removes `Vec::leak`, `check_resources`, `show_relative_to`, and every `std::fs` and `std::env` read outside `resource.rs` and the CLI; a grep for `std::fs` in `src/` outside `resource.rs` and `main.rs` is part of the acceptance. Tests: a memory resolver renders `samples/report.md` with its theme and images from memory; two jobs with a font file replaced between them embed different fonts; the renderer holds no face after a job whose layout was dropped. Byte guard.
3. **Diagnostics and cancellation.** Owns `src/diagnostic.rs`, `src/cancel.rs`, `src/layout/mod.rs`, `src/layout/pages.rs`, `src/pdf.rs`, `src/epub/mod.rs`, and `src/main.rs` for the printing moves. Adds the `Cancel` parameter to `layout::layout`, `pdf::write`, and `epub::write`. Tests: a token cancelled from another thread 50 ms into the corpus layout returns `Cancelled` within 30 ms of the cancel, measured, and the same renderer then lays the corpus out normally; the CLI's stderr and stdout for every sample are identical before and after. Byte guard. Runs in a worktree parallel to step 2, since the two own disjoint files except `pipeline.rs`, which step 2 owns and step 3 touches in one integration commit to pass the token through.
4. **Layout result and source map.** Owns `src/output.rs` (from `src/page.rs`), `src/layout/paragraph.rs`, `src/markdown.rs`, `src/markdown/comments.rs`, `src/document.rs`, `src/spacing.rs`, and new `src/source_map.rs`. Depends on step 2 for `Arc<Face>`. Tests: a `Layout` of the corpus renders to PDF after its input, resolver, and renderer are dropped; the PDF from `Layout::pdf` equals the PDF from `Renderer::render`; every text item of a mapped `samples/de.md` round-trips `position` then `locate` to itself; a hyphenated word maps its hyphen to `None` and its letters to the written bytes; a no-break-space document maps around the inserted spaces; the measured map size for the corpus is recorded in progress. Byte guard. Its parser and paragraph work can start in a worktree parallel to steps 2 and 3, and merges after step 2.
5. **CLI, desktop, examples, and parity.** Owns `src/main.rs`, `desktop/src/preview/`, `desktop/src/document.rs`, `desktop/src/theme_file/`, `examples/`, and `docs/`. The CLI uses only the public modules; the desktop painter takes an `Arc<Layout>`, the scheduler cancels instead of waiting, and the galley reads fonts from `Face::data`. The six documentation examples from the contract compile as `examples/` and run in CI. The parity test lays out every sample and the corpus through `render`, through `layout` then `pdf`, and through the CLI binary, and compares the PDFs byte for byte, which is stricter than the contract asks and is what the guard already proved. The CLI and the desktop halves can run in parallel worktrees; both wait for step 4.

The byte guard stays the acceptance of the whole phase. Runtime and memory of the corpus through the public API, with reused renderer state and four concurrent jobs, are measured in step 5 and recorded in progress against the phase 1 limits.

## Contradictions and recommendations

- **Implicit filesystem and environment access.** The contract says "The core library performs no implicit filesystem or network access." Today `date::Date::today` reads `SOURCE_DATE_EPOCH` and the clock, `theme_path::directory` reads the home directory and environment for the shared theme folder, `installed::system` scans system fonts, and `project::open` scans `manuscript/`. The desktop decisions depend on all four ("resolved by the engine", "installed lookup"). Recommendation: the date and the shared theme folder become inputs (`Input::today`, `ThemeDirs::shared`), installed fonts and folder listing go behind the resolver, and the CLI and app supply them. Behaviour is unchanged, and the engine is testable without a machine.
- **Parser internals the desktop needs.** The contract keeps "parser internals, intermediate document structures" private and excludes "editor components". The spike result lists "exposing the citation scanner the source layer now reaches through `druck::bibliography::syntax`" as a phase 2 need, and the desktop also uses `markdown::marks`, `config::merge`, and `config::layer::default_theme_value`. Recommendation: the `editor` feature of section 1, unstable and outside `API_VERSION`, rather than a public promise on parser syntax.
- **A layout cannot be stopped.** The spike decided "the running layout finishes before the newest text starts, because a layout cannot be stopped." The contract requires cooperative cancellation during layout. Section 8 supersedes the spike's scheduler; record that in decisions when step 3 lands.
- **Fonts leaked per file version.** The spike's `Loaded` comment says "user fonts are leaked once per version of a file" and the known limit is that a replaced font is not seen. The contract requires that "the renderer must not assume that a path or identifier always denotes unchanged bytes" and that a result "remain usable after the original request inputs and renderer are released". Sections 3 and 4 resolve both with content-hashed `Bytes` and `Arc<Face>`.
- **Byte-identical output.** The contract puts "byte-for-byte identical PDFs" outside the contract. The migration uses it as the guard anyway, because the samples already are byte-identical across platforms in CI and nothing in this design changes layout. This is stricter, not contradictory.
- **Raster preview and editor components** stay out of the crate, as the contract says. The desktop's `raster.rs` painter and `source/` layer stay in `druck-desktop`; the only engine-side additions they need are `Face::data` and the source map, which section 5 and 6 give them. No reason was found to move them.

## Summary of signatures

```rust
let renderer = Renderer::new();                                   // once per process
let resolver = FileResolver::new();                               // or MemoryResolver
let cancel = Cancel::default();
let input = Input { text: DocumentText::Single { .. }, theme: ThemeSelection::Document { dirs }, overrides, today };

// batch
let Rendered { bytes, warnings } = renderer.render(&resolver, &input, &PdfOptions::default(), &cancel)?;

// desktop, on each pause, on a worker thread with a fresh `cancel` and the old one cancelled
let configured = renderer.job(&resolver, &input, &cancel).configure()?;
let layout = Arc::new(configured.layout(&LayoutOptions { source_map: true })?);
// paint from `layout`, hit test with `layout.source_map()`, later `layout.pdf(&options)`
```
