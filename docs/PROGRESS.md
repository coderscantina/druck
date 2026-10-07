# Progress

## Current state

Milestone 07 is complete. GitHub pipe tables render with column alignment, inline styles, and wrapped cells. Column widths come from the shaped cell text: natural widths when they fit, otherwise a common width clamped between each column's minimum and natural width. A row is one line of the flow, so pages and columns break only between rows, and the header row repeats at the top of every page or column a table continues in. A `: Caption` paragraph after a table is its numbered caption, set above it with the `table` label. Tables take the column width inside `columns` and the text width inside `full-width`. Footnotes in body cells go on the page of their row. Oversized rows, words too wide for the narrowest columns, and unsupported content in cells are errors at their location.

Current milestone: none active.
Next milestone: [08: Document structures and templates](milestones/08-document-structures.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [x] 05: Mid-page column layouts.
- [x] 06: Images and captions.
- [x] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Tables are described in [decisions](DECISIONS.md#2026-10-07-milestone-07-multipage-tables). For milestone 08, figure and table numbers come from two counters in `Flow` (`src/layout/mod.rs`); cross-references need them before layout, so move numbering to a pass over the document model. `: Caption {#tbl:x}` and `![Caption](file){#fig:x}` are reserved and currently errors. Header repetition lives at the composer boundary as `pages::Table` in `src/layout/pages.rs`; running headers and page variants are separate work in `render`. Samples for visual checks are `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, `samples/images.md`, and `samples/tables.md`.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 102 unit and 17 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: tables parse with left, center, right, and default alignment, inline styles, cell locations, and padded short rows; a caption paragraph after a table becomes its caption; tables parse in columns, lists, and quotes; excess cells, images in cells, header footnotes, tables in footnotes, misplaced or second captions, caption labels, caption footnotes, and a caption without a blank line are reported at their location; footnotes in body cells are numbered in reading order. In layout: a short table keeps its natural width, centered, and wide columns share the rest equally; a word too wide for the narrowest columns is reported at its cell; a row is as tall as its tallest cell plus padding and rule; an 80-row table breaks between rows with the header on every page it continues on and its caption on the first; a table in columns repeats its header at the top of the second column; a full-width table sits between balanced columns that resume below; a note referenced in a cell stays on its row's page; a row too tall with the repeated header is reported at the row; tables and figures are numbered apart; a table in columns lays out identically twice. The sample render test now includes `samples/tables.md`. The CLI unsupported-content test uses strikethrough instead of a table, and one parser test case for tables was removed, since tables are supported now.
- Visual review: rendered `samples/tables.md` from `/tmp` with `kyber render <repo>/samples/tables.md -o a.pdf`, rasterized with `pdftoppm -r 80 -png`, and looked at all three pages. Page 1 has the small settings table at natural width, centered under its caption, and the long table filling the text width from its caption to the foot of the page, above the note of the paragraph before it. Page 2 starts with the repeated header, continues the long table with the note of the Linotype row at its foot, and starts the column section with the small table in the second column. On page 3 that table continues at the top of the first column under its repeated header; the long column table continues in the second column under its header; the full-width table spans the text area below balanced columns; the columns resume and end balanced. No row is split and nothing overlaps.
- `pdftotext` of the tables sample holds the text of all 200 table cells, each as a contiguous run. Two renders are byte-identical (`cmp`). All fonts are embedded.
- `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, and `samples/images.md` render byte for byte as with a build of the milestone 06 commit.
- Diagnostics checked through the CLI: a row of 3 000 words is reported at its line as 1911.9 pt high with the repeated header, and three 80-letter words in one table are reported at the widest one's cell. No PDF is written.

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter, three runs after a warm-up, median shown. The baseline is a release build of the milestone 06 commit, same day. The table inputs repeat the body of `samples/tables.md` with footnote labels made unique, and the long-table input is one table of 2 200 rows cycling the rows of the long table in that sample.

| Input | Pages | Time | Peak memory | Baseline |
| --- | --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 0.04 s | 12 MB | 0.04 s, 12 MB |
| `samples/en.md` body × 26 | 52 | 0.20 s | 25 MB | 0.20 s, 25 MB |
| `samples/en.md` body × 52 | 104 | 0.40 s | 41 MB | 0.40 s, 40 MB |
| `samples/de.md` body × 60 | 95 | 0.35 s | 35 MB | 0.34 s, 35 MB |
| `samples/tables.md` body × 17 | 47 | 0.17 s | 23 MB | not supported |
| `samples/tables.md` body × 34 | 94 | 0.32 s | 37 MB | not supported |
| one table of 2 200 rows | 86 | 0.32 s | 74 MB | not supported |

Text inputs are unchanged. The long table holds every cell's shaped text until its widths are known; setting rows frees them one by one, which brought it from 85 MB down to 74 MB. All inputs stay within the proposed limits: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- Page 2 of `samples/images.md` ends about a quarter page early. The text after the chart fits there, and that end is cheaper for page 2 alone, but the search prefers to keep page 3 fuller because the square fill cost spreads the shortfall that the tall image on page 4 forces. A scoring trade-off, not a composer defect; details in [decisions](DECISIONS.md#composer-gap-in-the-images-sample). Tables do not trigger it.
- A short table can split like a short paragraph. In `samples/tables.md` the four-row table in columns leaves its first row at the foot of page 2 (the 5 000 orphan cost) rather than leave the page short. A higher cost for breaking inside a table, or orphan and widow rules over two rows, is an option if review asks for it.
- A table caption is set across the frame width even when the table is narrower and centered. Setting it at the table width would align it with the table but wrap long captions more.
- Without floats, an image that does not fit leaves a short page, and the square fill cost may then prefer a widow over an even shorter page (page 1 of `samples/images.md`). A tall image in columns can leave the other column mostly empty (page 4). Both follow from document order; a scoring change needs care, see decisions.
- Raster images ignore stored density (PNG `pHYs`, JPEG JFIF), so high-resolution screenshots appear at one pixel per point unless scaled down. Reading density is a small addition if review asks for it.
- PNG images are decoded twice, once to validate and once by krilla while writing. Fine at document image sizes.
- Captions cannot hold footnotes, because CommonMark does not parse `[^note]` in an image description.
- An image in a keep group is not shrunk to fit the rest of the group; a group too tall is reported at its directive as before.
- Notes have no widow or orphan rules, so a continued note can leave one line on either page.
- A keep group where the even column split would fall can leave final columns visibly uneven, 5 lines against 14 in `samples/columns.md` at 35 mm margins. Both splits are about equally tall, and LaTeX's multicol picks the same one. A preference for a taller first column at near-equal height is an option if review asks for it.
- Columns with headings can end up to a line apart when their spaces cannot absorb the difference within bounds.
- The space at a change between one and two columns reuses `page.column-gap`. A separate theme setting is possible if review asks for it.
- A line hyphenated at the foot of a page costs nothing extra. TeX penalizes that; it showed up in the footnote benchmark.
- An introducing sentence ending in a colon can end a page while its list or code block starts the next. Authors can use `keep`; a small penalty after such lines is an option.
- Lists and code inside notes use the list and code styles at body size, not the footnote size.
- German compounds break at any pattern point, not preferably at compound boundaries. Better data would be needed.
- Only one glyph protrudes per line edge. A comma after a closing quote hangs; the quote does not.
- Breaking and page-scoring constants are internal. Expose them in themes only if visual review asks for it.
- Explicit hyphens at a line end are dropped by `pdftotext`. Marked content with actual text could fix extraction, if needed for accessibility work.
- Font bytes from disk are leaked once per render. Fine for the CLI; the phase 2 crate must own them (see decisions).
- `kyber check` does not parse the Markdown body, so unsupported content and directive or footnote errors only show up in `render`. Consider parsing in `check` too.
- Adjacent lists of different kinds and a list right after a paragraph get no space between them, because the default `list` style has zero `space-before`. A theme design question.
- Required title slots are not checked yet. Which title layout is active is a milestone 08 decision.
- `--set date=2024` parses as a number and fails; quoting works. Consider accepting numbers for text metadata.
- Front matter font settings take effect only if the theme uses the `body`, `heading`, and `mono` font tokens. A warning for unused settings might help.
- Paths in diagnostics are absolute and not normalized (`doc/../theme`).
- Displayed page numbering, `{section}` selection, and required-slot timing are open for milestone 08.

## Updating this file

Replace the resume note with the latest completed work and next action. Record exact verification commands and results, remaining criteria, and blockers. Check a milestone only after its completion gate passes. Preserve useful follow-ups without copying the product requirements or decision history here.
