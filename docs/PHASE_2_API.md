# Phase 2 API

The public Rust API of Druck as it shipped on 2026-11-04, for [the phase 2 contract](RUST_CRATE_API_PHASE_2.md). It began as a design written on 2026-11-03 against the engine after milestone 15 and the [desktop spike results](DECISIONS.md#2026-11-03-desktop-spike-results). This version is a record of what was built: signatures are the real ones, and where the build differs from the design, the reason is in the dated entries of [decisions](DECISIONS.md) for steps [1](DECISIONS.md#2026-11-03-phase-2-step-1-choices-beyond-the-design), [2](DECISIONS.md#2026-11-03-phase-2-step-2-choices-beyond-the-design), [3](DECISIONS.md#2026-11-04-phase-2-step-3-choices-beyond-the-design), [4](DECISIONS.md#2026-11-04-phase-2-step-4-choices-beyond-the-design), and [5](DECISIONS.md#2026-11-04-phase-2-step-5-choices-beyond-the-design). [Acceptance](ACCEPTANCE.md#phase-2-contract) lists the evidence for each contract requirement. The rustdoc is the reference for every item: `cargo doc --no-deps --open`.

Two clients drove every choice: the desktop app, which lays out the same document again on each pause with one set of fonts, and a batch render in a CLI or a service, which lays out once and writes bytes.

## 1. Crate layout

One package, `druck`, with a library target and a binary target. The library is the public crate. The binary is the CLI and uses only the library's public modules, which the compiler enforces: a binary target is a separate crate and cannot see `pub(crate)` items. `grep -nE 'editor|doc\(hidden\)' src/main.rs` finds nothing, and `cargo install --path . --locked` builds it. The desktop stays the workspace member `druck-desktop` and depends on `druck` by path with the `editor` feature.

```toml
[features]
default = ["cli"]
cli = ["dep:clap"]   # the binary's own dependency
editor = []          # unstable module for the desktop, outside API_VERSION

[[bin]]
name = "druck"
required-features = ["cli"]
```

One package keeps `cargo install druck`, `scripts/release.sh`, `scripts/notices.py`, `build.rs`, and the release workflow unchanged. A library user who does not turn off default features compiles clap, which `default-features = false` avoids: `cargo build --no-default-features --lib` has no clap in its tree.

The public modules, as `cargo doc --no-deps` lists them:

| Module | Holds |
| --- | --- |
| root | `Renderer`, `Job`, `Configured`, `Rendered`, `Failure`, `LayoutOptions`, `PdfOptions`, `EpubOptions`, `API_VERSION`, `THEME_SCHEMA_VERSIONS` |
| `input` | `Input`, `DocumentText`, `ThemeSelection`, `ThemeDirs`, `Overrides`, `Project`, `ExportSettings`, `Export`, `Date`, `MANIFEST` |
| `resource` | `Resolver`, `FileResolver`, `MemoryResolver`, `Bytes`, `ResolveError`, `InstalledFace`, `Origin`, `Resource`, `normalize`, `source_date`, `clock_date` |
| `output` | `Layout` and everything it returns: `Page`, `Item`, `Run`, `Glyph`, `Face`, `FaceId`, `Picture`, `Pixels`, `Link`, `Structure`, `Rect`, `Position`, `Bookmark`, `PageNumber`, `Hit`, `Located`, `Color`, `Pt`, and the source map types |
| `diagnostic` | `Diagnostic`, `Severity`, `Code`, `PageName`, `Source`, `Location`, `Origin`, `Resource`, `path_from` |
| `cancel` | `Cancel` |

Private: the parser, `document`, citations, `layout` and its scoring, `tags`, `pdf`, `epub`, `pipeline`, `config` (the resolved `Config` is opaque, inspected as JSON), `text`, `installed`, `theme_path`, `highlight`.

**The `editor` module** is `#[doc(hidden)]`, behind the feature, and exempt from `API_VERSION`. It holds only what the desktop's source layer and theme file code cannot do without the engine's internals, each with a one-line reason in `src/editor.rs`:

| Item | Why the desktop needs it |
| --- | --- |
| `syntax` | The source layer must tint exactly what the parser reads as a citation. |
| `marks` | The source layer must pair `^x^`, `~x~`, and `{todo:` as the parser does. |
| `theme_value::{default_theme_value, merge}` | The theme file code edits and saves partial themes over the bundled default. |
| `theme_path` | The theme file code lists, finds, and creates named themes. |
| `resolved_config::{resolve, Inputs, SettingsInput, ThemeInput, InputSource, FrontMatter}` | The theme validator resolves a theme as a render does. |
| `resolved_config::{Config, FontStyle, Weight, of}` | The preview reads the body style, title, and authors from the resolved configuration. |
| `engine::{document_theme, theme_file}` | The Styles tab edits the file a selection or a document means. |

Step 5 removed `engine::face_of` (now `Layout::font`), `document_text`, `FontFiles`, `Face`, `Lang`, and `Color` from this list.

## 2. Entry paths

```rust
pub struct Renderer { /* caches, section 4 */ }

impl Renderer {
    pub fn new() -> Self;
    /// The simple path: Markdown and configuration in, PDF bytes and every warning out.
    pub fn render(&self, resolver: &dyn Resolver, input: &Input, options: &PdfOptions, cancel: &Cancel)
        -> Result<Rendered, Failure>;
    /// The explicit path. A job is one document on one thread.
    pub fn job<'r>(&'r self, resolver: &'r dyn Resolver, input: &'r Input, cancel: &'r Cancel) -> Job<'r>;
}

impl Job<'_> {
    /// Resolves and checks the configuration and reads every resource it names. Errors here never reach layout.
    pub fn configure(self) -> Result<Configured<'_>, Failure>;
}

impl Configured<'_> {
    pub fn warnings(&self) -> &[Diagnostic];
    pub fn config_json(&self) -> String;                       // what `druck check --print-config` prints
    pub fn check_final(&self, export: &str) -> Result<(), Failure>;   // open placeholders of a final export
    pub fn layout(&self, options: &LayoutOptions) -> Result<Layout, Failure>;
    pub fn epub(&self, options: &EpubOptions) -> Result<Rendered, Failure>;   // needs no layout
}

impl Layout {
    /// Writes the PDF from the display list alone. Nothing is shaped, paginated, or read again.
    pub fn pdf(&self, options: &PdfOptions, cancel: &Cancel) -> Result<Rendered, Failure>;
}

pub struct Rendered { pub bytes: Vec<u8>, pub warnings: Vec<Diagnostic>, pub parts: usize }  // pages, or EPUB documents
pub struct LayoutOptions { pub source_map: bool }                   // default false
pub struct PdfOptions { pub bleed: Pt, pub crop_marks: bool }       // export only, never changes layout
pub struct EpubOptions { pub modified: Option<Date> }
pub enum Failure { Cancelled, Errors(Vec<Diagnostic>) }              // Display, Error, From<Vec<Diagnostic>>
```

`render` is `job(..).configure()?`, `layout(&LayoutOptions::default())?`, `pdf(options, cancel)`, plus the configuration and layout warnings in `Rendered::warnings`, so both paths resolve identical inputs identically by construction. `config_json` is a pretty `String` because `serde_json` is built without `preserve_order` and a `Value` would sort the keys.

Inputs are values, with origins instead of paths, so a service can render from memory:

```rust
pub struct Input {
    pub text: DocumentText,
    pub theme: ThemeSelection,
    pub overrides: Overrides,
    pub today: Date,        // the caller reads the clock and SOURCE_DATE_EPOCH: resource::{clock_date, source_date}
}

pub enum DocumentText {
    /// One Markdown file with front matter. `origin` is an `Origin::Document`, the folder its paths start from.
    Single { text: String, origin: Origin, name: String },
    /// A project: joined chapters without front matter, settings from the manifest.
    Project(Box<Project>),
}
impl DocumentText { pub fn read(resolver: &dyn Resolver, path: &Path) -> Result<Self, Vec<Diagnostic>>; }

pub enum ThemeSelection {
    Document { dirs: ThemeDirs },                                  // the document's own `theme` setting
    Name { name: String, base: PathBuf, dirs: ThemeDirs },         // project, then a local file, then shared
    File { path: PathBuf },
    Json { text: String, origin: Origin },                         // in memory, assets relative to `origin`
}
impl ThemeSelection { pub fn asked(selection: &Path, base: &Path, dirs: ThemeDirs) -> Self; }   // what `--theme` means

pub struct ThemeDirs { pub project: Option<PathBuf>, pub shared: Option<PathBuf> }   // no environment is read

impl Overrides {
    pub fn parse(pairs: &[String], working_dir: &Path) -> Result<Self, Vec<Diagnostic>>;          // --set syntax
    pub fn from_json(value: serde_json::Value, working_dir: &Path) -> Result<Self, Vec<Diagnostic>>;
}

impl Project {
    pub fn open(resolver: &dyn Resolver, dir: &Path) -> Result<Self, Vec<Diagnostic>>;
    pub fn warnings(&self) -> &[Diagnostic];                                    // chapters not in the book
    pub fn exports(&self) -> impl Iterator<Item = (&str, &Export)>;
    pub fn export(self, name: Option<&str>) -> Result<(DocumentText, ExportSettings), Vec<Diagnostic>>;
}
pub struct ExportSettings { pub pdf: PdfOptions, pub is_final: bool, pub output: Option<PathBuf>, pub print_conflict: Option<Diagnostic> }
```

The typed configuration the contract asks for is these inputs plus validation. The resolved `Config` stays opaque. Exposing its forty fields would put the whole schema under the API version, and the only typed reader is the desktop, which gets it through `editor`. Project folders are in the crate, as `input::Project`, because the CLI, the app, and a service that receives a folder need the same manifest rules. Choosing the output path and creating folders stay in the client.

## 3. Resource resolver

```rust
pub trait Resolver: Send + Sync {
    /// The bytes of `path` relative to `origin`.
    fn read(&self, origin: &Origin, path: &str) -> Result<Bytes, ResolveError>;
    /// The files below `dir`, for a project's `manuscript/`. `None` where listing is not supported.
    fn list(&self, origin: &Origin, dir: &str) -> Option<Vec<String>> { None }
    /// The faces of an installed family, for themes that name fonts without files. The default finds none.
    fn installed(&self, family: &str) -> Result<Vec<InstalledFace>, ResolveError> { Ok(Vec::new()) }
}
pub enum ResolveError { NotFound(String), Unreadable(String), Unsupported }
pub struct Bytes { /* krilla::Data and a content hash */ }       // Deref<[u8]>, From<Vec<u8>>, From<Arc<[u8]>>
pub struct FileResolver;     // FileResolver::new(), FileResolver::shared_themes() -> Option<PathBuf>
pub struct MemoryResolver;   // MemoryResolver::new(), insert(origin, path, impl Into<Arc<[u8]>>)
```

`Origin` is `Bundled`, `Theme(dir)`, `Document(dir)`, `WorkingDir(dir)`, or `Installed`. Bundled fonts and the default theme never reach the resolver, so a `MemoryResolver` with nothing in it renders a plain document. The core performs no discovery: installed fonts, folder listing, and the shared theme folder are the caller's, and `FileResolver` provides them. Every read failure is a diagnostic with the resource, its origin, the property that named it, and for document images the line.

Changed contents are found by content, not by path. Every job reads every resource it uses through the resolver and keys reuse by `Bytes::hash`. A file that is replaced under the same path is seen on the next job. Cost: an unchanged theme's fonts are read and hashed again, which the 203-page book does in about 1 ms of its 375 ms.

## 4. Reusable renderer and font ownership

`Renderer` keeps two tables across jobs: faces by `(content hash, face index, variation coordinates)`, and decoded images by content hash up to 256 MB. Each job takes the next number and, when it ends and was not cancelled, the next one drops what neither of the two jobs before it used. A `Layout` holds `Arc<Face>` and `Picture` clones, so it keeps its own after the renderer drops them. Shape plans live in the `Face` and survive across jobs. `Vec::leak` and the static font table are gone.

Sharing is enforced by types, with compile-time assertions in `src/renderer.rs`:

- `Renderer`, `Layout`, `Input`, `Cancel`, `FileResolver`, and `MemoryResolver` are `Send + Sync`.
- `Job` and `Configured` are `Send` and not `Sync`: one job, one thread. A second assertion fails to compile if either becomes `Sync`.
- The crate starts no threads and needs no async runtime. A layout needs a stack of about 1 MB in a debug build, so a client on a small thread (a Windows main thread, a pool thread) uses one of 8 MB or more.

A PDF export gives each of its faces a font handle of its own, because krilla clones a font per glyph and the clones of one shared handle made concurrent exports 3 to 11 times slower. See [the step 5 entry](DECISIONS.md#2026-11-04-phase-2-step-5-choices-beyond-the-design).

## 5. Layout result

```rust
impl Layout {
    pub fn pages(&self) -> &[Page];
    pub fn face(&self, id: FaceId) -> &Face;                              // for Run::face
    pub fn font(&self, family: &str, weight: u16, italic: bool) -> Option<&Face>;   // a theme family's face, as emphasis picks it
    pub fn faces(&self) -> impl Iterator<Item = &Arc<Face>>;
    pub fn anchor(&self, id: AnchorId) -> Position;
    pub fn outline(&self) -> &[Bookmark];
    pub fn page_numbers(&self) -> &[Option<PageNumber>];
    pub fn warnings(&self) -> &[Diagnostic];                              // typesetting rules the layout breaks
    pub fn source_map(&self) -> Option<&SourceMap>;
    pub fn hit(&self, page: usize, x: Pt, y: Pt) -> Option<Hit>;          // the text nearest a point
    pub fn position_at(&self, page: usize, x: Pt, y: Pt) -> Option<SourcePos>;
    pub fn locate(&self, at: SourcePos) -> Option<Located>;               // page, item, and rectangle
    pub fn pdf(&self, options: &PdfOptions, cancel: &Cancel) -> Result<Rendered, Failure>;
}

pub struct Page { pub width: Pt, pub height: Pt, pub items: Vec<Item> }

pub enum Item {
    Text { x: Pt, y: Pt, run: Run, color: Color, source: Option<TextId> },
    TurnedText { x: Pt, y: Pt, angle: f64, run: Run, color: Color },
    Rect { rect: Rect, color: Color },
    Link { rect: Rect, link: Link },
    Image { rect: Rect, picture: Picture },
    Anchor { id: AnchorId, x: Pt, y: Pt },
    Begin(Structure), End,                    // structure boundaries: not drawn
}

pub struct Run { pub text: String, pub face: FaceId, pub size: Pt, pub glyphs: Vec<Glyph>, pub width: Pt /* and spacing, read by layout only */ }
pub struct Glyph { pub id: u16, pub x_advance: Pt, pub x_offset: Pt, pub y_offset: Pt, pub text: Range<usize> }
impl Face { pub fn data(&self) -> &Bytes; pub fn index(&self) -> u32; pub fn coordinates(&self) -> &[([u8; 4], f32)]; }
impl Picture { pub fn size(&self) -> (Pt, Pt); pub fn source(&self) -> &Bytes; pub fn pixels(&self) -> Pixels<'_>; }
pub enum Pixels<'a> { Raster { width: u32, height: u32 }, Svg(&'a usvg::Tree) }
pub enum Link { Url(String), Anchor(AnchorId) }
```

Documented in `output.rs`, which is the reference for painters: points, origin at the top left of the page, y growing down. `Page::width` and `height` are the trim size; bleed and crop marks exist only in `PdfOptions`. Items draw in order, later over earlier. A run draws from `(x, y)` on the baseline: for each glyph, the outline of `id` in `face.data()` at `face.index()` with `face.coordinates()`, scaled by `size / units_per_em`, at `(pen + x_offset, y - y_offset)`, then `pen += x_advance`. `TurnedText` turns the run `angle` degrees counterclockwise about `(x, y)`. `Glyph::text` is the byte range of `Run::text` the glyph covers, empty for the second and later glyphs of a cluster. An image fills `rect`. `Link` rectangles and `Anchor` points are not drawn.

PDF export is the old `pdf::write` with the faces taken from the layout. It paginates nothing, shapes nothing, and resolves nothing: `Face::space` is a cmap lookup done once when a face is built. `Picture::pixels` gives the size of a raster and the tree of an SVG, not RGBA samples, because the engine keeps krilla's decoded image and holding a second copy would double the memory of every image; `Picture::source` is the file, which painters decode themselves. `Structure` is the engine's tag mark, re-exported, with an opaque `StructureId`.

## 6. Source mapping

A text item holds a `TextId` (4 bytes, in the padding of the item), and the `SourceMap` holds the rest, so an item is 112 bytes with or without a map. The map is opt-in through `LayoutOptions::source_map`, and the parser's range bookkeeping is skipped when it is off.

```rust
pub struct SourcePos { pub chapter: ChapterId, pub offset: u32 }     // a byte offset in a chapter's file as written
pub struct ChapterId(pub u32);                                       // 0 for a single file
pub struct Chapter { pub path: PathBuf }
pub struct BlockSource { pub id: BlockId, pub kind: BlockKind, pub parent: Option<BlockId>, pub range: (SourcePos, u32) }
pub enum BlockKind { Paragraph, Heading(u8), List, ListItem, Quote, Callout, Code, Image, Table, Footnote, SceneBreak, Directive }
pub struct TextSource { pub block: BlockId, pub spans: Vec<Span> }   // built on request by SourceMap::text
pub struct Span { pub run: Range<u32>, pub source: Option<SourcePos> }   // None for inserted text

impl SourceMap {
    pub fn chapters(&self) -> &[Chapter];
    pub fn blocks(&self) -> &[BlockSource];
    pub fn text(&self, id: TextId, run: &Run) -> TextSource;
    pub fn position(&self, id: TextId, byte: usize) -> Option<SourcePos>;   // exact, None for inserted or generated text
    pub fn nearest(&self, id: TextId, byte: usize) -> SourcePos;            // what a click wants
    pub fn locate(&self, at: SourcePos) -> Option<(usize, usize)>;          // page and item
    pub fn byte_of(&self, id: TextId, at: SourcePos, length: usize) -> usize;
    pub fn memory(&self) -> usize;                                           // bytes the map holds
}
```

Encoding and semantics are in the rustdoc of `SourceMap`. The corpus's map is 0.68 MB and adds 2.6 percent to layout time ([step 4](DECISIONS.md#2026-11-04-phase-2-step-4-choices-beyond-the-design)).

## 7. Diagnostics

```rust
pub struct Diagnostic {
    pub severity: Severity,                 // Error | Warning
    pub message: String,
    pub code: Option<Code>,                 // what the message says, with its values, for a client's own wording
    pub source: Option<Source>,             // Bundled | Theme(PathBuf) | Document(PathBuf) | Bibliography(PathBuf) | Overrides
    pub location: Option<Location>,         // 1-based line and column in `source`; in a project, in the chapter file
    pub property: Option<String>,           // dotted theme or setting path, "styles.body.size"
    pub resource: Option<Resource>,         // the font, image, or file concerned, with its origin
}
impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self;
    pub fn is_warning(&self) -> bool;
    pub fn display_from<'a>(&'a self, base: &'a Path) -> impl Display + 'a;   // for terminals: paths under `base` shortened
}
```

`Failure::Errors` carries every error of the stage that failed, in reading order. Warnings sit beside success: `Configured::warnings`, `Project::warnings`, `Layout::warnings`, and `Rendered::warnings`. The engine never prints and never exits. The CLI prints exactly what it printed before; the byte guard and a command matrix check that.

## 8. Cancellation

`Cancel` is `Clone + Default + Send + Sync`, with `cancel()` and `is_cancelled()`. A job looks at it before each resource read, face parse, citation, bibliography parse, and image decode; at each layout pass and block; every 64 lines of the page break search; and at each page of a PDF and chapter of an EPUB. A cancelled job returns `Failure::Cancelled` from the stage it was in, never partial bytes or a partial layout, and the renderer keeps what it had. Measured on the corpus in release builds, through the public API: a cancel 50 ms into a layout returns `Cancelled` in a median of 0.78 ms (maximum 1.1 ms, 25 cancels), and across all of step 3's 88 cancels the median was 1.3 ms and the maximum 19.4 ms, which falls in parsing before the first block. `Layout::pdf` takes a token of its own because a layout outlives its job.

## 9. Compatibility and versioning

The package version stays CalVer, so it says nothing about compatibility. `druck::API_VERSION` (1) is raised on any breaking change to the public modules, and a release that keeps it keeps every public signature. `druck::THEME_SCHEMA_VERSIONS` (`[1]`) lists the theme `version` values this release reads; any other fails in `configure` with a diagnostic at the `version` property. The two change independently. `druck.yaml` has no version. Private and free to change: everything not listed in section 1, the hash algorithm of `Bytes`, the cache eviction rule, the shape plan key, layout scoring, the tag tree, and the `editor` module.

## 10. Migration, as it ran

The guard for every step was a release build rendering every sample and the book before and after, with `cmp` showing byte-identical PDFs and EPUBs, plus the standard checks.

1. **Fence the surface:** facade, `cli` and `editor` features.
2. **Resolver and owned fonts:** `Resolver`, content-hashed `Bytes`, `Arc<Face>`, no `Vec::leak`, no reads outside `resource.rs` and the CLI.
3. **Diagnostics and cancellation:** `Diagnostic` with sources, properties, and resources; `Cancel` and its check points.
4. **Layout result and source map:** `output::Layout`, the source map, click sync in the app.
5. **CLI, desktop, examples, and parity:** the CLI through public modules only; `editor` cut to what the desktop needs; six examples run in CI; parity and public API tests; the contract's acceptance table; measurements.

## What the design got wrong or left out

- **Implicit filesystem and environment access.** The date, the shared theme folder, installed fonts, and folder listing became inputs (`Input::today`, `ThemeDirs::shared`) and resolver methods. The CLI and the app supply them.
- **A layout cannot be stopped.** The spike's scheduler waited for the running layout. It now cancels it and starts the newest text.
- **Fonts leaked per file version.** Content-hashed `Bytes` and `Arc<Face>` replaced the leak, and a replaced font is seen.
- **Byte-identical output.** The contract leaves it outside; the migration and the parity test use it anyway, because the same engine gave it on macOS. Other platforms are unverified, since CI has not run.
- **Raster preview and editor components** stay out of the crate. The desktop's painter and source layer are in `druck-desktop`.
- **Sizes in the design.** Section 6 estimated the map at a run per line fragment; a run is a word fragment. The stored form differs and the estimate held ([step 4](DECISIONS.md#2026-11-04-phase-2-step-4-choices-beyond-the-design)).
