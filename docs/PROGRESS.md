# Progress

## Current state

Milestone 04 is complete. Page breaks are chosen for the whole document by a scored search over break positions and placed footnote lines. Headings stay with their text, widows and orphans are avoided, space between blocks stretches within bounds, and `keep` and `page-break` directives work. Footnotes are numbered by reference, set at the foot of their reference's page, take part in break selection, and continue on the next page under a marker. `columns` and `full-width` are parsed and reported as not supported yet.

Current milestone: none active.
Next milestone: [05: Mid-page column layouts](milestones/05-columns.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [ ] 05: Mid-page column layouts.
- [ ] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Pagination and footnotes are described in [decisions](DECISIONS.md#2026-10-07-milestone-04-pagination-and-footnotes). For milestone 05, `Block::Columns` and `Block::FullWidth` already come out of the parser with their checks; layout reports them in `Flow::blocks` in `src/layout/mod.rs`. The composer in `src/layout/pages.rs` plans each page as a list of regions plus one footnote range; columns add a region kind and need column sections to enter the flow as units whose height comes from balancing. Samples for visual checks are `samples/en.md`, `samples/de.md`, and `samples/pagination.md`.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 62 unit and 15 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: directives parsed between blocks and not in code; each directive error at its line; footnotes numbered in reference order with definitions anywhere; each footnote error; a heading near the bottom moving with its text; widow and orphan avoidance; a keep group moving whole and an explicit break holding; notes in order on their reference pages; a note moving the break before its reference; a long note continuing page after page; raised markers, notes below the rule, the "(continued)" marker, and no body text below the rule or past the text area; a keep group taller than the page reported at its directive. The sample render test now includes `samples/pagination.md`. Two parser tests changed because footnotes and directives are no longer reported as unsupported.
- Visual review: rendered `samples/pagination.md` with `cargo run -q -- render samples/pagination.md -o /tmp/...` at the default 28 mm margins and with `--set margins=20mm` and `35mm`, rasterized with `pdftoppm -r 80` and `-r 50`, and looked at all four pages of each. Headings stay with their text at all three margins. The keep group moves whole. Notes sit at the foot of their reference's page in order, below the rule, with no overlap. At 28 mm, note 3 starts at the foot of page 1 and continues on page 2 under "³ (continued)". At 35 mm, note 5 continues with a single line, which is allowed (see follow-ups). The page before the explicit break ends short, and the break holds. Re-checked `samples/en.md` and `samples/de.md` at `-r 60`: the German "Seiten" heading, which milestone 02 left alone at the foot of page 1, now moves with its text. Pages 5 and 6 of the footnote-heavy benchmark input look orderly.
- `pdftotext` of the pagination sample has every word of the source, plus "continued" once. Two renders at each margin are byte-identical (`cmp`).

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter. Each input ran three times on this build and twice on a build of the milestone 03 commit, after a warm-up, with the same results each time. The footnote input repeats the English body 40 times with a two-line note on each of its 720 paragraphs.

| Input | Pages | Time | Peak memory | Milestone 03 build, same day |
| --- | --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 0.04 s | 12 MB | 10 pages, 0.04 s, 12 MB |
| `samples/en.md` body × 26 | 52 | 0.19 s | 24 MB | 50 pages, 0.19 s, 24 MB |
| `samples/en.md` body × 52 | 104 | 0.38 s | 40 MB | 99 pages, 0.38 s, 39 MB |
| `samples/de.md` body × 60 | 95 | 0.32 s | 35 MB | 93 pages, 0.32 s, 34 MB |
| `samples/en.md` body × 40, 720 notes | 107 | 0.46 s | 47 MB | not supported |

Scored pagination adds no measurable time. Pages run about 5 percent shorter on average to avoid stranded lines and headings. All inputs stay within the proposed limits: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB. Revisit them after milestone 07.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- Notes have no widow or orphan rules, so a continued note can leave one line on either page.
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
