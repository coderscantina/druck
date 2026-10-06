# Progress

## Current state

Milestone 03 is complete. Paragraphs are broken with paragraph-wide total-fit optimization on shaped widths, hyphenated in English and German from bundled patterns, justified by spacing only, and set with optical margin alignment. Pagination is still milestone 02's first fit.

Current milestone: none active.
Next milestone: [04: Scored pagination and footnotes](milestones/04-pagination.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [ ] 04: Scored pagination and footnotes.
- [ ] 05: Mid-page column layouts.
- [ ] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Paragraph composition and its parameters are in [decisions](DECISIONS.md#2026-10-06-milestone-03-paragraph-composition). Milestone 04 replaces `paginate` in `src/layout/mod.rs`, which receives a flat list of lines and spaces. Paragraph lines carry no grouping yet, so widow and orphan control needs to know where a paragraph's lines start and end; heading retention needs the same for headings. Samples for visual checks are `samples/en.md` and `samples/de.md`.

Remaining criteria carried into later milestones: heading retention, widows and orphans, scored page breaks (04).

## Verification

On 2026-10-06, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 51 unit and 15 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: a breaker case where greedy filling strands a word and the paragraph-wide choice does not; English and German hyphenation with the text preserved; no hyphens with `hyphenate: false` or in code; hyphenation across a bold and regular boundary keeping each style; justified lines meeting the margins exactly once protrusion is subtracted, at four widths; repeated layout identical; hyphenation rules for punctuation, compounds, acronyms, URLs, and explicit hyphens.
- Visual review: rendered both samples with `cargo run -q -- render samples/<lang>.md --set margins=<m> -o /tmp/...` at 15 mm, 28 mm (default), and 40 mm margins, rasterized with `pdftoppm -r 80` and `-r 100`, and inspected the first pages, plus 200 dpi crops of both text edges. Spacing is even at the default and wide measures. The 40 mm measure and the narrow quotation have a few loose lines where no better breaks exist. Hyphens appear at sensible points in both languages ("fol-low", "empha-sized", "Meilen-stein", "Silben-trennung", "Kraftfahrzeughaft-pflichtversicherung"). Hyphens, commas, periods, and German quotes visibly hang into the margins. Headings stay ragged and unhyphenated.
- `pdftotext` returns the text with hyphenated words rejoined, ligatures and umlauts intact. It also drops the explicit hyphen of "CommonMark-Spezifikation" when that ends a line (see decisions). Two renders of the same input are byte-identical (`cmp`).

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter, two runs each with the same results:

| Input | Pages | Time | Peak memory |
| --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 0.04 s | 13 MB |
| `samples/en.md` body × 26 | 50 | 0.19 s | 25 MB |
| `samples/en.md` body × 52 | 99 | 0.39 s | 41 MB |
| `samples/de.md` body × 60 | 90 | 0.31 s | 35 MB |

Milestone 02's greedy filler took 0.33 s and 38 MB for 95 pages, so paragraph-wide breaking with hyphenation costs about 15 percent. Time and memory grow linearly with pages.

Proposed limits on this hardware, for release builds, with room for footnotes, tables, images, and scored pagination still to come: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB. Revisit them after milestones 04 and 07, which change the workload most.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- German compounds break at any pattern point, not preferably at compound boundaries. Better data would be needed.
- Only one glyph protrudes per line edge. A comma after a closing quote hangs; the quote does not.
- Breaking constants (stretch, shrink, penalties, tolerance) are internal. Expose them in themes only if visual review asks for it.
- Explicit hyphens at a line end are dropped by `pdftotext`. Marked content with actual text could fix extraction, if needed for accessibility work.
- Font bytes from disk are leaked once per render. Fine for the CLI; the phase 2 crate must own them (see decisions).
- `kyber check` does not parse the Markdown body, so unsupported content only shows up in `render`. Consider parsing in `check` too.
- Adjacent lists of different kinds and a list right after a paragraph get no space between them, because the default `list` style has zero `space-before`. A theme design question.
- Required title slots are not checked yet. Which title layout is active is a milestone 08 decision.
- `--set date=2024` parses as a number and fails; quoting works. Consider accepting numbers for text metadata.
- Front matter font settings take effect only if the theme uses the `body`, `heading`, and `mono` font tokens. A warning for unused settings might help.
- Paths in diagnostics are absolute and not normalized (`doc/../theme`).
- Displayed page numbering, `{section}` selection, and required-slot timing are open for milestone 08.

## Updating this file

Replace the resume note with the latest completed work and next action. Record exact verification commands and results, remaining criteria, and blockers. Check a milestone only after its completion gate passes. Preserve useful follow-ups without copying the product requirements or decision history here.
