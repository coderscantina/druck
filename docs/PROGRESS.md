# Progress

## Current state

Milestone 09 is complete. Documents cite works from a local BibTeX file with `[@key]`, grouped keys, page locators, and narrative `@key` citations, in a built-in author-date or numeric style in English or German. A bibliography of the cited works follows at the end, or where `::: bibliography` stands, also in two columns. It is in the table of contents and the outline, and every citation links to its entry. Decisions are in [decisions](DECISIONS.md#2026-10-07-milestone-09-citations-and-bibliography); the syntax, styles, and fields are in [authoring](AUTHORING.md#citations-and-bibliography).

Current milestone: none active.
Next milestone: [10: Release acceptance and distribution](milestones/10-release-acceptance.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [x] 02: First text-to-PDF rendering path.
- [x] 03: Optimized paragraph composition.
- [x] 04: Scored pagination and footnotes.
- [x] 05: Mid-page column layouts.
- [x] 06: Images and captions.
- [x] 07: Multipage tables.
- [x] 08: Document structures and templates.
- [x] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

For milestone 10: release groundwork is merged ([decisions](DECISIONS.md#2026-10-07-milestone-10-release-groundwork), [release procedure](RELEASE.md)). CI now renders every sample on macOS, Linux, and Windows and fails if the PDFs differ, checks that `THIRD_PARTY_NOTICES.md` is current (run `python3 scripts/notices.py` after dependency changes), and `release.yml` builds five targets as artifacts without publishing. None of these workflows has run, since nothing is pushed. Still open for 10: Kyber's own license (the owner's decision; distribution is blocked until then), the visual acceptance review, cross-platform results, final benchmarks against the limits, and the list of internal boundaries for phase 2. Samples for the acceptance review are `samples/report.md` (also with `--theme samples/themes/report.json` and `--set citation-style=numeric`), `samples/report-de.md`, `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, `samples/images.md`, and `samples/tables.md`. The first follow-up below shows in two of the report renders and may be worth fixing before a release.

## Verification

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 162 unit and 23 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: the parser reads bracketed, grouped, multi-line, and narrative citations with locators and the location of every key, leaves citations in link text as text, adds the bibliography at the end, and keeps a `::: bibliography` inside columns; it reports an unreadable group, a group mixing a citation and a cross-reference in either order, a citation in a heading, a second directive, and one inside `keep`, each at its location. In layout: citations read "(Smith and Jones 2024, p. 12; Lee et al. 2022)" and "Weber (2020, pp. 3–5)", or "[1, p. 12; 2]" and "Weber [3, pp. 3–5]", next to a cross-reference, and link to the anchor on their entry's first line; every entry type of the fixture is listed once, sorted by name in author-date and by first citation in numeric, with a hanging indent, a top-level bookmark, and a contents entry with its final page; the directive sets the bibliography in two columns, and a URL too wide for a column is reported at its line in the `.bib` file; layout with citations is identical twice. The CLI renders both reports in both styles with a bookmarked bibliography and identically twice, and reports a missing `.bib` file by property, a missing required field at its `.bib` line, a citation without a bibliography, and a missing key at its location.
- Changed tests: `@smith2024` in the cross-reference test is now a citation, and the unknown directive message names `bibliography`. In the bibliography module, a group mixing a cross-reference key with a citation key is now invalid instead of text, locators contain a no-break space, an English numeric edition reads "2nd ed." instead of "2 ed.", `.bib` errors carry `Source::Bibliography`, and a missing key names its item instead of a byte range. Layout test helpers pass the formatted citations.
- Visual review: rendered from `/tmp/kyber09/final` with `kyber render <repo>/samples/report.md -o report.pdf`, the same with `--theme <repo>/samples/themes/report.json` and with `--set citation-style=numeric`, and `samples/report-de.md` in both styles, rasterized with `pdftoppm -r 80 -png`, and looked at all 50 pages (pages that compared pixel-identical across styles once). Citations sit in prose, in the two-column section, in footnotes, at a column break ("Schäfer (2018, / S. 40–52)"), and after the long table at a page end. The English bibliography starts under the conclusion and continues on the next page with "References" in the running header; the German one fills two balanced columns after a `::: bibliography` inside `columns`. Author-date entries are sorted by name with "Knuth 1984a" and "1984b"; numeric ones follow first citation, and "[@tschichold1928; @bringhurst2004]" reads "[1, 3]". German entries show "Hrsg.", "3. Aufl.", "Masterarbeit", and "Abgerufen am". Two entries break badly, see the first follow-up.
- Milestone 08 scenario: in all five PDFs every contents entry, including the bibliography, names the page its heading is on, checked with `pdftotext` per page, and the page references read page 6, 7, and 8 for targets on those pages. `pdftotext` shows all 15 entries in every bibliography and no unresolved `@key` outside inline code. The default English report has 69 internal links and the bibliography as a top-level bookmark. Repeated renders of all five are byte-identical (`cmp`).
- Other samples: `en`, `de`, `pagination`, `columns`, `images`, `tables`, and the milestone 08 `report.md` in both themes render byte-identical to a release build of the milestone 08 commit.

Benchmarks with `cargo build --release` and `/usr/bin/time -l`, sample body repeated after its front matter with labels and footnote labels made unique, three runs after a warm-up, median shown. The baseline is a release build of the milestone 08 commit on the same input, same day. Report inputs use the current samples, with 21 citations per copy; the German one keeps one `::: bibliography`. The cited English input adds a grouped and a narrative citation to every prose paragraph of `samples/en.md`, 1 560 in all.

| Input | Pages | Passes | Time | Peak memory | Baseline |
| --- | --- | --- | --- | --- | --- |
| `samples/en.md` body × 52 | 103 | 1 | 0.42 s | 42 MB | 0.42 s, 41 MB |
| `samples/tables.md` body × 34 | 93 | 1 | 0.31 s | 38 MB | 0.31 s, 38 MB |
| milestone 08 `report.md` body × 16 | 101 | 2 | 0.39 s | 31 MB | 0.39 s, 32 MB |
| `samples/report.md` body × 6, author-date | 46 | 2 | 0.20 s | 22 MB | not supported |
| `samples/report.md` body × 12, author-date | 90 | 2 | 0.39 s | 34 MB | not supported |
| `samples/report.md` body × 16, author-date | 119 | 2 | 0.51 s | 40 MB | not supported |
| `samples/report.md` body × 16, numeric | 119 | 2 | 0.50 s | 37 MB | not supported |
| `samples/report-de.md` body × 16, author-date | 119 | 2 | 0.53 s | 39 MB | not supported |
| `samples/report-de.md` body × 16, numeric | 119 | 2 | 0.53 s | 42 MB | not supported |
| `samples/en.md` body × 52 with citations, author-date | 111 | 1 | 0.49 s | 51 MB | not supported |
| `samples/en.md` body × 52 with citations, numeric | 111 | 1 | 0.45 s | 48 MB | not supported |

Pass counts came from a temporary instrumented build. Citations and the bibliography add no pass: documents with a table of contents settle in two, others in one. All inputs stay within the proposed limits of 2 s and 150 MB for 100 pages.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- When no first line of a ragged paragraph fits within tolerance, the breaker's second pass caps badness at 10 000, so all very loose first lines tie and the earliest break wins. A bibliography entry whose URL does not fit after the first line can then break after its first word: "Knuth," alone in the custom-theme report, "The Unicode Standard, Version / 16.0" in the German numeric one. Uncapped badness in the second pass, or an emergency stretch, in [the breaker](../src/layout/paragraph/breaking.rs) would fix it; it changes milestone 03 behaviour, so it was left.
- URLs and DOIs never break, so one wider than a column is an error at its `.bib` entry. Allowing breaks after `/` in link text that spells its URL would make two-column bibliographies safer.
- In both reports, page 3 ends about 40% short and page 4 starts with the last line of a paragraph, because the longer text before the column section no longer fits on page 3. A pagination scoring trade-off before a column region, not caused by citations; the milestone 08 version of the sample laid out fully.
- Every citation links to one entry, the first work it shows. Per-work links inside a group would need per-item output from `Rendered`.
- Numeric entries put their label at the start of the first line; there is no label column, so text after "[10]" starts later than after "[9]".
- `kyber check` reads neither the Markdown body nor the `.bib` file, so citation and BibTeX errors only show up in `render`.
- A bracket that does not start with `@`, such as `[see @key, p. 3]`, is text with a narrative citation inside, not a citation with a prefix as in Pandoc.
- The `type` field of a report or thesis prints as written, so an English `type` reads English in a German document.
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
