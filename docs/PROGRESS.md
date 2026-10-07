# Progress

## Current state

Milestone 08 is complete. Documents get a title block from their metadata or a separate title page, numbered headings, a table of contents with dot leaders and final page numbers, running headers and page numbers from the theme's page variants, and labelled cross-references to headings, figures, and tables. References and contents entries are internal links, and headings are PDF bookmarks. Page numbers shown in the text settle by repeating layout, usually in two passes. Decisions are in [decisions](DECISIONS.md#2026-10-07-milestone-08-document-structures-and-templates).

Current milestone: none active.
Next milestone: [09: Citations and bibliography](milestones/09-citations.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [x] 05: Mid-page column layouts.
- [x] 06: Images and captions.
- [x] 07: Multipage tables.
- [x] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

For milestone 09: citations take the `@key` and `[@key]` forms that cross-reference labels leave free, since every label has a `sec:`, `fig:`, or `tbl:` prefix. In [the parser](../src/markdown.rs) they belong where `reference_label` finds no prefix; today such text stays as it is. A bibliography is generated content in the flow and must be laid out in every pass of `settle` in [layout](../src/layout/mod.rs), like the table of contents in [toc.rs](../src/layout/toc.rs). The theme's `references` label is unused so far. Citation groundwork is merged in [src/bibliography/](../src/bibliography/mod.rs): BibTeX reader, citation syntax `syntax::find`, the `Citations` collector, and both styles in English and German, with 41 unit tests. It is not declared in `main.rs` yet. Its API and open points are in [decisions](DECISIONS.md#2026-10-07-milestone-09-citation-groundwork). What remains for 09: declare the module, call `find` on text outside code and links in the parser, report invalid and mixed groups, add `Source::Bibliography`, load the `.bib` resource, and lay out the bibliography inside `settle`. Re-run the milestone 08 scenario below with bibliography content. Samples for visual checks are `samples/report.md` (also with `--theme samples/themes/report.json`), `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, `samples/images.md`, and `samples/tables.md`.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 116 unit and 19 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: labels on headings, figures, and tables and references in both forms parse, while `@key`, `\@fig:x`, and `a@fig:x` stay text; wrong prefixes, heading classes, duplicate labels, figure labels without a caption, undefined labels, references in headings and links, and other bracket content are reported at their location. In layout: an omitted optional title slot leaves the same gap as a theme without it, and `*Plain*` metadata stays literal; a title page stands alone, the body starts on page 2 with the inner margin on the right, and the footer counts from the title page; a missing required title slot is reported by property, and a document without metadata gets no title block; variants follow title, first, odd, even (null), body; headers show `{section}` and `{subsection}` per page and footers `{page}`, and a required header slot without a value names the page; headings are numbered to `numbering-depth` and bookmarked by level at their first line; a 40-part document's two-page table of contents lists every entry with its final page and links; references show "Section 2", "Figure 1", "Table 1", and the final page, linked to their targets; settling stops after a confirming pass, needs one pass when nothing is shown, and reports an anchor that keeps moving after 5 passes at its location; layout with contents and references is identical twice. PDF output nests bookmarks by level and writes link destinations. The CLI renders the report in both themes with bookmarks and internal links and identically twice, and `check` reports a required title slot without a value. Template filling has a unit test.
- Changed tests: layout test helpers ignore text on the header and footer baselines, and two tests that require all text inside the text area allow those baselines, because the default theme now draws page numbers and running headers. A table caption ending in `{#tbl-a}` now reports the invalid label instead of "not supported yet". Tests construct the new `label` fields and the `Output` type.
- Visual review: rendered `samples/report.md` from `/tmp/kyber08/final` with `kyber render <repo>/samples/report.md -o report.pdf`, and with `--theme <repo>/samples/themes/report.json -o custom.pdf`, rasterized with `pdftoppm -r 80 -png`, and looked at all 16 pages. Default theme: a centered title page with subtitle, authors, date, and an abstract under its heading, no header or footer; the contents on page 2 with aligned leaders and the `first` variant's centered page number; body pages with the section in the header and the page number in the footer; the two-column section with notes from both columns, a full-width figure between balanced column regions, two more figures, a short and a long table whose header repeats on page 7, and footnotes. Every reference reads the page its target starts on: Section 5 on page 7, Table 2 on page 6, Figure 3 on page 5, and the contents agree with the headings. The fourth-level heading is unnumbered and referenced by its text. Custom theme: a left-aligned title page with the theme's logo image, no leaders in the contents, mirrored margins, the section at the right of odd headers and the title at the left of even ones, page numbers outside, and "Fig." and "§" in references.
- Navigation: each report PDF has an outline with 6 top-level and 12 bookmarks in all, nested by level, 17 XYZ destinations (12 headings, 3 figures, 2 tables), and 35 (default) or 34 (custom) link annotations, all with `/Dest` and none with `/URI`. `pdftotext` shows the generated reference texts. Links were not clicked in a viewer.
- Repeated renders of both reports are byte-identical (`cmp`).
- Existing samples: their duplicate `# Title` heading was removed and the other headings promoted by one level, since the title block now shows the title. All six render with a title block, numbered headings, running headers, and page numbers; all 18 pages were inspected. To check that the layout itself is unchanged, rendering the milestone 07 versions of the samples with a theme that turns these structures off (no title slots, bands, or numbering) gives pixel-identical pages to a build of the milestone 07 commit, all 18 compared with `cmp` on the PNGs.

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter, three runs after a warm-up, median shown. The baseline is a release build of the milestone 07 commit on the same inputs, same day. Inputs are built from the current samples, so their headings are promoted and the title block is shown. Report inputs repeat the body of `samples/report.md` with labels and footnote labels made unique; the 101-page one has 192 headings, 176 contents entries, and 384 references.

| Input | Pages | Passes | Time | Peak memory | Baseline |
| --- | --- | --- | --- | --- | --- |
| `samples/en.md` body × 5 | 10 | 1 | 0.04 s | 13 MB | 0.04 s, 12 MB |
| `samples/en.md` body × 26 | 51 | 1 | 0.20 s | 25 MB | 0.20 s, 25 MB |
| `samples/en.md` body × 52 | 103 | 1 | 0.40 s | 41 MB | 0.41 s, 40 MB |
| `samples/de.md` body × 60 | 94 | 1 | 0.34 s | 36 MB | 0.34 s, 35 MB |
| `samples/tables.md` body × 17 | 47 | 1 | 0.16 s | 23 MB | 0.16 s, 23 MB |
| `samples/tables.md` body × 34 | 93 | 1 | 0.31 s | 37 MB | 0.32 s, 37 MB |
| one table of 2 200 rows | 86 | 1 | 0.33 s | 83 MB | 0.33 s, 81 MB |
| `samples/report.md` body × 6 | 39 | 2 | 0.15 s | 20 MB | not supported |
| `samples/report.md` body × 12 | 76 | 2 | 0.29 s | 28 MB | not supported |
| `samples/report.md` body × 16 | 101 | 2 | 0.39 s | 32 MB | not supported |

Pass counts came from a temporary instrumented build. Documents without a table of contents or page references take one pass and cost the same as before. The reports take two: a guess and a confirming pass. All inputs stay within the proposed limits: 10 pages within 0.2 s and 40 MB, 50 pages within 1 s and 80 MB, 100 pages within 2 s and 150 MB.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- A reference's label can hyphenate at a line end ("Fig-ure 1" in the report sample), since label and number form one word. A rule against breaking inside reference text is an option.
- `{section}` shows the first section that starts on a page, so a page that opens in the middle of section 2 and starts section 3 shows section 3. This is LaTeX's right mark; a theme choice of the section in effect at the top of the page is an option if review asks for it.
- After a title page the body starts on an even page. Duplex printing would want a blank page so it starts on the right; there is no setting for that.
- The heading number is followed by one space. A theme setting for the gap, like LaTeX's quad, is an option.
- All contents entries use one style. Bold top-level entries would need per-level styles.
- Settling has no damping; a document whose page numbers oscillate is reported after 5 passes. Every pass shapes everything again; caching paragraphs without page references would make the second pass cheaper if larger documents need it.
- Headings inside footnotes are set unnumbered and left out of the contents and outline. Rejecting them in the parser is an option.
- Every theme image is decoded on render, also ones no active slot uses.
- Page 2 of `samples/images.md` ends about a quarter page early. The text after the chart fits there, and that end is cheaper for page 2 alone, but the search prefers to keep page 3 fuller because the square fill cost spreads the shortfall that the tall image on page 4 forces. A scoring trade-off, not a composer defect; details in [decisions](DECISIONS.md#composer-gap-in-the-images-sample). Tables do not trigger it.
- A short table can split like a short paragraph. A higher cost for breaking inside a table, or orphan and widow rules over two rows, is an option if review asks for it.
- A table caption is set across the frame width even when the table is narrower and centered. Setting it at the table width would align it with the table but wrap long captions more.
- Without floats, an image that does not fit leaves a short page, and the square fill cost may then prefer a widow over an even shorter page (page 1 of `samples/images.md`). A tall image in columns can leave the other column mostly empty. Both follow from document order; a scoring change needs care, see decisions.
- Raster images ignore stored density (PNG `pHYs`, JPEG JFIF), so high-resolution screenshots appear at one pixel per point unless scaled down. Reading density is a small addition if review asks for it.
- PNG images are decoded twice, once to validate and once by krilla while writing. Fine at document image sizes.
- Captions cannot hold footnotes, because CommonMark does not parse `[^note]` in an image description.
- An image in a keep group is not shrunk to fit the rest of the group; a group too tall is reported at its directive as before.
- Notes have no widow or orphan rules, so a continued note can leave one line on either page.
- A keep group where the even column split would fall can leave final columns visibly uneven. Both splits are about equally tall, and LaTeX's multicol picks the same one. A preference for a taller first column at near-equal height is an option if review asks for it.
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
- `kyber check` does not parse the Markdown body, so unsupported content, directive, footnote, and label errors only show up in `render`. Consider parsing in `check` too.
- Adjacent lists of different kinds and a list right after a paragraph get no space between them, because the default `list` style has zero `space-before`. A theme design question.
- `--set date=2024` parses as a number and fails; quoting works. Consider accepting numbers for text metadata.
- Front matter font settings take effect only if the theme uses the `body`, `heading`, and `mono` font tokens. A warning for unused settings might help.
- Paths in diagnostics are absolute and not normalized (`doc/../theme`).

## Updating this file

Replace the resume note with the latest completed work and next action. Record exact verification commands and results, remaining criteria, and blockers. Check a milestone only after its completion gate passes. Preserve useful follow-ups without copying the product requirements or decision history here.
