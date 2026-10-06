# Progress

## Current state

Milestone 02 is complete. `kyber render` parses CommonMark, shapes text with the bundled Libertinus fonts, lays it out with temporary greedy line filling and first-fit page breaks, and writes a PDF with embedded subset fonts, searchable text, and clickable links. Unsupported content is an error with its location.

Current milestone: none active.
Next milestone: [03: Optimized paragraph composition](milestones/03-paragraphs.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [ ] 03: Optimized paragraph composition.
- [ ] 04: Scored pagination and footnotes.
- [ ] 05: Mid-page column layouts.
- [ ] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

The pipeline and its rules are in [decisions](DECISIONS.md#2026-10-06-milestone-02-rendering-path). Milestone 03 replaces the greedy filler in `src/layout/paragraph.rs`: it receives shaped words with their spaces as tokens, so a paragraph-wide breaker can replace the fill loop while keeping the positioning code. It needs hyphenation data for English and German, bundled like the fonts. Samples for visual checks are `samples/en.md` and `samples/de.md`.

Remaining criteria carried into later milestones, not completed here: paragraph-wide line breaking, hyphenation, optical margin alignment (03); heading retention, widows and orphans, scored page breaks (04).

## Verification

On 2026-10-06, macOS 27 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 41 unit and 15 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- Integration tests render both samples from a separate working directory and check embedded fonts and link annotations, a font file relative to the document, the default output path, unsupported content reported at its line with nothing written, and a too-wide code line reported at its line.
- Visual review: rendered `samples/en.md` and `samples/de.md` and inspected every page at 80 dpi. Headings, indents, lists with nested bullets and start numbers, italic quotation with upright emphasis, code, link color, umlauts, and quotation marks are correct. Greedy justification gives visibly loose lines in the narrow quotation, as expected before milestone 03.
- `pdftotext` returns the German text with ligature words and umlauts intact. `pdffonts` lists four embedded subset fonts with ToUnicode maps. `pdfinfo` shows title and author. Two renders of the same input are byte-identical.

Baseline with `cargo build --release`, English sample body repeated (two runs each, same results):

| Pages | Time | Peak memory |
| --- | --- | --- |
| 10 | 0.03 s | 12 MB |
| 48 | 0.16 s | 24 MB |
| 95 | 0.33 s | 38 MB |

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

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
