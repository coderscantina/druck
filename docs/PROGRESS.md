# Progress

## Current state

Milestone 06 is complete. Local PNG, JPEG, and SVG images render from Markdown image syntax, relative to the document. An image alone in its paragraph is a figure; its description is the numbered caption in `styles.caption`. Images shrink proportionally to the column or text width and to the page height, never grow, and stay with their caption: when they do not fit, both move to the next column or page. `full-width` gives an image the whole text width inside columns. Missing, unreadable, malformed, unsupported, and remote images are errors at their line, naming the file.

Current milestone: none active.
Next milestone: [07: Multipage tables](milestones/07-tables.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [x] 05: Mid-page column layouts.
- [x] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Images are described in [decisions](DECISIONS.md#2026-10-07-milestone-06-images-and-captions). For milestone 07, `Flow::image` in `src/layout/mod.rs` is the pattern: content enters the flow as lines, and lines marked `Break::Never` stay together through pagination, column splits, and balancing in `src/layout/pages.rs`. Table rows can be lines that never split inside, with header repetition as the new part. Captions go through the same path with the `table` label. Tables are still reported as unsupported by the parser in `src/markdown.rs`. Samples for visual checks are `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, and `samples/images.md`.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 87 unit and 17 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: each format decodes with its size in points; malformed PNG and SVG, an SVG referring to another file, JPEG data named `.png`, GIF, and an unknown extension are rejected with their messages; PNG, JPEG, and SVG items write the same PDF bytes twice; images parse alone in a paragraph with the description as caption and one entry per distinct file; text around an image, images in headings, links, and footnotes, remote and `data:` URLs, empty paths, titles, and unparsed image syntax are reported at their location; proportional downscaling to the width and height without growing, centered; captions numbered with the theme label, skipping uncaptioned images; image and caption moving together to the next page and to the next column at the column width; a full-width image between balanced column regions; a note staying on the page of its reference beside an image; a tall image shrinking to fit below its heading; a caption leaving no room reported at the image; rendering images relative to the document from another working directory, twice byte for byte; missing, malformed, unsupported, mismatched, and remote images reported by `render` with line and file. The sample render test now includes `samples/images.md`. One parser test case and the CLI unsupported-content test changed from images to tables, since images are supported now.
- Visual review: rendered `samples/images.md` from `/tmp` with `kyber render <repo>/samples/images.md -o a.pdf`, rasterized with `pdftoppm -r 80 -png`, and looked at all five pages. Page 1 has the 240 by 150 pt PNG at natural size, centered, its caption, and the note of the next paragraph at the foot. The oversized SVG moves to page 2 with its caption, scaled to the text width; page 1 ends short and the last line of the paragraph before it goes along (see follow-ups). The SVG chart is scaled to the column width in the second column of page 2. On page 3 the JPEG portrait moves to the top of the second column with its caption, at natural size, and the full-width PNG spans the text area below balanced columns. On page 4 the tall SVG is scaled to the column height with its caption at the foot of the second column. Columns resume and end balanced on page 5. No overlaps. `pdfimages -list` shows the PNG embedded once for two references, at 72 ppi.
- `pdftotext` of the images sample reads in source order with every word of the text, the captions, and the SVG labels. Two renders are byte-identical (`cmp`).
- `samples/en.md`, `samples/de.md`, `samples/pagination.md`, and `samples/columns.md` render byte for byte as with a build of the image backend commit, whose layout is the milestone 05 layout.
- Benchmark inputs with images, rasterized at `-r 40`: the first pages of the single-column and column inputs show images scaled, captioned, and kept in order.

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter, three runs after a warm-up, median shown. The baseline is a release build of the image backend commit, same day. The image inputs put a figure after every third paragraph of `samples/en.md`, cycling through the PNG, SVG chart, JPEG, and oversized SVG of `samples/images/`, 156 figures over 26 repetitions. The column variant puts each repetition after its first heading in a `columns` section, with the code line shortened to fit a column.

| Input | Pages | Time | Peak memory | Baseline |
| --- | --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 0.04 s | 13 MB | 0.04 s, 13 MB |
| `samples/en.md` body × 26 | 52 | 0.20 s | 25 MB | 0.20 s, 25 MB |
| `samples/en.md` body × 52 | 104 | 0.41 s | 41 MB | 0.40 s, 41 MB |
| `samples/de.md` body × 60 | 95 | 0.33 s | 36 MB | 0.34 s, 36 MB |
| `samples/en.md` body × 26 with 156 figures | 105 | 0.20 s | 27 MB | not supported |
| the same in columns | 66 | 0.23 s | 27 MB | not supported |

Text inputs are unchanged. The figure inputs are fast because each of the four files is loaded once, and an image is a single line for the composer. All inputs stay within the proposed limits: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

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
