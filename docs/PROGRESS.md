# Progress

## Current state

Milestone 05 is complete. `columns` sections set text in two equal columns with `page.column-gap` between them, flowing down the first column, then the second, then across pages. Every column region is balanced, and the next block starts below the taller column. `full-width` blocks balance the columns before them and resume columns below. Footnotes from both columns share the full-width area at the foot of the page. Keep groups stay in one column, and a keep group taller than a column is reported at its directive.

Current milestone: none active.
Next milestone: [06: Images and captions](milestones/06-images.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [x] 05: Mid-page column layouts.
- [ ] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Columns are described in [decisions](DECISIONS.md#2026-10-07-milestone-05-column-layouts). For milestone 06, an image inside `columns` gets the column frame from `Flow::columns` in `src/layout/mod.rs`, so `Frame` and `Flow::width` give the width to scale to; inside `full-width` it gets the text width. The composer in `src/layout/pages.rs` works on lines: an image with its caption can enter the flow as lines marked `Break::Never` except the last, and balancing and pagination then treat it like a keep group. Images are still reported as unsupported by the parser in `src/markdown.rs`. Samples for visual checks are `samples/en.md`, `samples/de.md`, `samples/pagination.md`, and `samples/columns.md`.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 71 unit and 15 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: text flowing down the first column then the second; balanced final columns for two, three, and five paragraphs; a section continuing across pages with both columns full on every page but the last; a full-width block below balanced columns with columns resuming below it; a change from one to two columns and back on one page; notes referenced from both columns in one area in reference order; a keep group staying in one column where the even split would cut it; a keep group that fits at full width reported at its directive when taller than a column; repeated column layout identical; `columns` inside `keep` reported by the parser. The sample render test now includes `samples/columns.md`. No existing test expectation changed.
- Visual review: rendered `samples/columns.md` with `cargo run -q -- render samples/columns.md -o /tmp/...` at the default margins and with `--set margins=20mm` and `35mm`, rasterized with `pdftoppm -r 80` and `-r 60`, and looked at both pages of each. Page 1 changes from one to two columns and back, then starts the long section; the section continues on page 2, where a full-width block interrupts it and columns resume below. The short section's columns end within a line of each other, and the next paragraph starts below the taller one with the column gap. Notes 1 and 2 come from the first column and note 3 from the second, all in one area below the rule. The keep group stays whole in one column. At 35 mm the first region on page 2 is 5 lines against about 14, because the keep group cannot split and both possible splits are about equally tall (see follow-ups). No text overlaps at any margin.
- `pdftotext` of the columns sample reads in source order, column by column, and has every word of the source. Two renders at each margin are byte-identical (`cmp`).
- `samples/en.md`, `samples/de.md`, and `samples/pagination.md` render byte for byte as on the milestone 04 commit.
- Column benchmark inputs, rasterized at `-r 45`: pages 1 to 4 of the column input with notes and page 2 of the plain one show balanced regions, one-two-one-two changes on a page, a note continued under its marker, and lists split across columns.

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter. Each input ran three times on this build and three times on a release build of the milestone 04 commit, after a warm-up; the median is shown. The note inputs append a two-line note to every long paragraph line, 560 notes in all, so they differ from the milestone 04 note input. The column inputs put each repetition after its first heading in a `columns` section with the "Pages" heading and its first paragraph as a `full-width` block; the code line is shortened to fit a column.

| Input | Pages | Time | Peak memory | Milestone 04 build, same day |
| --- | --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 0.04 s | 12 MB | 10 pages, 0.04 s, 12 MB |
| `samples/en.md` body × 26 | 52 | 0.19 s | 25 MB | 52 pages, 0.19 s, 24 MB |
| `samples/en.md` body × 52 | 104 | 0.39 s | 40 MB | 104 pages, 0.38 s, 40 MB |
| `samples/de.md` body × 60 | 95 | 0.33 s | 35 MB | 95 pages, 0.33 s, 35 MB |
| `samples/en.md` body × 40, 560 notes | 100 | 0.46 s | 44 MB | 100 pages, 0.44 s, 43 MB |
| `samples/en.md` body × 52 in columns | 91 | 0.46 s | 40 MB | not supported |
| `samples/en.md` body × 40 in columns, 560 notes | 91 | 0.50 s | 44 MB | not supported |

Single-column inputs run up to 5 percent slower than before, from the region bookkeeping per candidate page end. Column inputs take about 20 percent longer than the same text in one column, mostly from setting more, shorter lines. All inputs stay within the proposed limits: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

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
