# Progress

## Current state

Milestone 09 is complete. Documents cite works from a local BibTeX file with `[@key]`, grouped keys, page locators, and narrative `@key` citations, in a built-in author-date or numeric style in English or German. A bibliography of the cited works follows at the end, or where `::: bibliography` stands, also in two columns. It is in the table of contents and the outline, and every citation links to its entry. Decisions are in [decisions](DECISIONS.md#2026-10-07-milestone-09-citations-and-bibliography); the syntax, styles, and fields are in [authoring](AUTHORING.md#citations-and-bibliography).

Milestones 11 and 12 are complete and merged. The [business documents amendment](BRIEFING.md#business-documents-amendment) of 2026-10-07 added milestones 11 to 13; release acceptance (10) waits until they are complete. Themes can now use installed fonts (looked up by the CLI only, collection faces included) with weights 100 to 900, tracking, capitals, keep-with-next, and custom styles applied with `{.name}` ([decisions](DECISIONS.md#2026-10-07-milestone-11-installed-fonts-and-custom-styles)). They can set prose narrower than wide blocks, unmirrored margins, title pages and bands made of anchored slot groups, `meta` values, and `{pages}` ([decisions](DECISIONS.md#2026-10-07-milestone-12-page-geometry-covers-and-bands)).

Milestone 13 is complete. A `::: table` directive holds a list table whose cells hold paragraphs and lists, with `{span=n}` cells, `{.name}` on rows and cells, and `align` and `widths` attributes where `*` columns fill the frame. Themes set `top-rule`, `header-rule`, and `row-rule`, and a row style's `rule-below` drops or changes the rule below its row. Title and band slots may name custom styles, and bands apply tracking and capitals. A wide table narrower than the frame starts with the prose instead of centering across the frame ([decisions](DECISIONS.md#2026-10-07-milestone-13-rich-tables), [authoring](AUTHORING.md#list-tables)).

The follow-up fixes of 2026-10-07 closed the open follow-ups from milestones 03 to 13 or settled them as [decisions](DECISIONS.md#2026-10-07-follow-up-fixes).

Placeholders, statistics, and drafts were added on 2026-10-07 at the owner's request, outside the milestone plan. Documents use `{words}`, `{chars}`, `{build-date}`, and the other placeholders that do not depend on the page in their text; slots gain the statistics, `{section-page}`, and `{section-pages}`; `draft: true` sets a LaTeX-style diagonal watermark from the new theme `watermark` section ([decisions](DECISIONS.md#2026-10-07-placeholders-statistics-and-drafts), [authoring](AUTHORING.md#placeholders)).

Current milestone: [10: Release acceptance](milestones/10-release-acceptance.md).

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
- [x] 11: Installed fonts and custom styles.
- [x] 12: Page geometry, covers, and bands.
- [x] 13: Rich tables.

## Resume note

Next: milestone 10, release acceptance. The project is now named Druck and MIT licensed, and `scripts/release.sh` cuts CalVer releases with a changelog and a GitHub release ([decisions](DECISIONS.md#2026-10-07-name-license-and-releases), [release](RELEASE.md)). Milestones 11 to 13 are complete; `samples/offer.md` with `samples/themes/business.json` now shows all three and joins the acceptance review.

For milestone 10: release groundwork is merged ([decisions](DECISIONS.md#2026-10-07-milestone-10-release-groundwork), [release procedure](RELEASE.md)). CI now renders every sample on macOS, Linux, and Windows and fails if the PDFs differ, checks that `THIRD_PARTY_NOTICES.md` is current (run `python3 scripts/notices.py` after dependency changes), and `release.yml` builds five targets as artifacts without publishing. None of these workflows has run, since nothing is pushed. Still open for 10: the visual acceptance review, cross-platform results, final benchmarks against the limits, and the list of internal boundaries for phase 2. Samples for the acceptance review are `samples/report.md` (also with `--theme samples/themes/report.json` and `--set citation-style=numeric`), `samples/report-de.md`, `samples/en.md`, `samples/de.md`, `samples/pagination.md`, `samples/columns.md`, `samples/images.md`, and `samples/tables.md`. The first follow-up below shows in two of the report renders and may be worth fixing before a release.

## Verification

Rename to Druck, license, and releases, on 2026-10-07 on the same machine: `cargo test -q` passed 245 unit and 29 integration tests, and `cargo clippy --all-targets -q -- -D warnings`, `cargo fmt --check`, and `python3 scripts/notices.py --check` were clean. No tracked file mentions the old name. The release build prints `druck 26.10.7-2b4381c` for `--version`, and `scripts/package.py` wrote `druck-26.10.7-2b4381c-aarch64-apple-darwin.tar.gz` with `LICENSE`, and its checksum verified. `scripts/release.sh` ran twice in a scratch clone against a local bare remote: both runs bumped the version, wrote the changelog, committed, and pushed distinct tags for the same day. All samples rendered with `scripts/render-samples.sh`; the themed report's title page shows the redrawn "D" logo. The README quick start renders as written. The workflows parse as YAML but have not run.

Placeholders, statistics, and drafts, on 2026-10-07 on the same machine: `cargo test -q` passed 245 unit and 29 integration tests, and `cargo clippy --all-targets -q`, `cargo fmt --check`, and `python3 scripts/notices.py --check` were clean; `unicode-segmentation` is the one new dependency. New tests cover counting (grapheme clusters, hyphenated words, abbreviations and German ordinals, generated text and placeholders counting as nothing, which blocks count as prose), reading time, the build date from Unix times, digit grouping, placeholder parsing and its errors, body values and missing values, `{section-page}` in footers, the watermark on every page with bands and shrunk on A5, and the theme's angle and `{abstract}` checks. `scripts/render-samples.sh` with debug builds of `e825c1e` and of the new tree gave byte-identical PDFs for all 11 renders. A scratch draft with every statistic, `SOURCE_DATE_EPOCH=1791331200`, and a footer with `{draft} · {build-date}` and `{section} {section-page}/{section-pages}` was rendered and inspected after `pdftoppm -r 50 -png`: "DRAFT" in light gray at 45 degrees, centered behind the text on both pages, "Draft · 7 October 2026" and "1 Introduction 1/1" in the footer, and counts that match a count by hand (53 words, 7 sentences, 3 paragraphs).

Owner follow-ups, on 2026-10-07 on the same machine, from `a153a39`: `cargo test -q` passed 231 unit and 29 integration tests, and `cargo clippy --all-targets -q` and `cargo fmt --check` were clean. New tests cover `check` reporting an unbreakable word without writing a PDF, quotations and custom lists scaled in a footnote, citation suffixes in the parser and in both styles, and a caption over a narrow table. `scripts/render-samples.sh` with release builds of `a153a39` (rendered from its own samples) and of the new tree: only `offer.pdf`, `report.pdf`, and `tables.pdf` changed. The offer (still 5 pages) shows smaller muted detail bullets on pages 2 to 4; `tables.pdf` page 3 has the caption of the narrow Table 3 on one line; `report.pdf` differs only in the extracted text of a line-end "full-" (plain hyphen again), with no visual change. All were inspected after `pdftoppm -r 80 -png` or compared with `pdftotext -layout`. The decisions are in [decisions](DECISIONS.md#owner-follow-ups).

Follow-up fixes, on 2026-10-07 on the same machine: four branches merged onto `0823f59`, plus a fix for list items that took the list's block spacing. `cargo test -q` passed 228 unit and 28 integration tests, and `cargo clippy --all-targets -q`, `cargo fmt --check`, and `python3 scripts/notices.py --check` were clean. Sample PDFs changed on purpose (pagination rules, list spacing, caption widths, label column, URL breaks); all samples were rendered with `scripts/render-samples.sh` from the merged tree, and the changed pages were inspected per branch and again after merging: the offer (5 pages, after dropping one sample paragraph that left two lines on a sixth page), `en`, `typography`, the numeric German bibliography, and a scratch document with a list and code in a footnote. Benchmarks per branch stayed within 10% of the earlier numbers (`samples/en.md` ×52 0.36 to 0.37 s, `samples/tables.md` ×34 0.29 s).

Milestone 13, on 2026-10-07 on the same machine: `cargo test -q` passed 199 unit and 23 integration tests; `cargo clippy --all-targets -q` and `cargo fmt --check` were clean; no dependency changed. `scripts/render-samples.sh` with a release build of `31c1e4f` and of the milestone 13 tree, compared with `cmp`, gave byte-identical PDFs for all renders except `offer.pdf`, which changed on purpose (new content, and wide tables now start with the prose). New tests cover parsing list tables with attributes, spans, row and cell styles, block cells, and captions; ten parser errors at their locations; slot styles naming custom styles, an unknown slot style, and `rule-below` on a list style; schema cases for the rules and slot styles; and in layout `*` and `auto` widths with column and cell alignment, the theme rules and their opt-out, a spanning cell widening its columns, a repeated header over three pages, a kept group row, wide placement, and band capitals with tracking. The 34-copy `samples/tables.md` benchmark (93 pages) took 0.30 s and 42 MB against 0.29 s and 40 MB for `31c1e4f`, with identical output. `samples/offer.md` (5 pages) was rasterized with `pdftoppm -r 80 -png` and every page inspected: cover labels in tracked capitals, eyebrows kept with their headings, the cost table over pages 2 to 4 with its header in small spaced capitals repeated, group rows kept with their first item, muted detail lists, the note row, totals without rules, and narrow wide tables flush with the prose. The reference offer comparison with installed Avenir Next is recorded in [decisions](DECISIONS.md#reference-comparison).

Milestones 11 and 12, on 2026-10-07 on the same machine: after merging both, `cargo test -q` passed 189 unit and 23 integration tests, and `cargo clippy --all-targets -q` and `cargo fmt --check` were clean. `scripts/render-samples.sh` with a release build of the amendment commit (`a4fe54e`, its own script and samples) and of the merged tree gave byte-identical PDFs for all nine existing renders. Avenir Next and Helvetica Neue embedded from the system `.ttc` files with the right six faces, and a misspelled family or a missing face was an error with no PDF written. `samples/offer.md` (4 pages) and `samples/typography.md` (2 pages) were rendered from the merged tree and inspected: the cover's metadata block and title, the three-column footer with "Page 1/4", narrow prose beside a frame-wide table, eyebrows kept with their headings, ✔ and • lists, and hanging step numbers.

The rest of this section records milestone 09.

On 2026-10-07, macOS 27.0.1 on an Apple M1 Pro with 32 GB:

- `cargo test -q`: 162 unit and 23 integration tests passed. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean.
- New tests: the parser reads bracketed, grouped, multi-line, and narrative citations with locators and the location of every key, leaves citations in link text as text, adds the bibliography at the end, and keeps a `::: bibliography` inside columns; it reports an unreadable group, a group mixing a citation and a cross-reference in either order, a citation in a heading, a second directive, and one inside `keep`, each at its location. In layout: citations read "(Smith and Jones 2024, p. 12; Lee et al. 2022)" and "Weber (2020, pp. 3–5)", or "[1, p. 12; 2]" and "Weber [3, pp. 3–5]", next to a cross-reference, and link to the anchor on their entry's first line; every entry type of the fixture is listed once, sorted by name in author-date and by first citation in numeric, with a hanging indent, a top-level bookmark, and a contents entry with its final page; the directive sets the bibliography in two columns, and a URL too wide for a column is reported at its line in the `.bib` file; layout with citations is identical twice. The CLI renders both reports in both styles with a bookmarked bibliography and identically twice, and reports a missing `.bib` file by property, a missing required field at its `.bib` line, a citation without a bibliography, and a missing key at its location.
- Changed tests: `@smith2024` in the cross-reference test is now a citation, and the unknown directive message names `bibliography`. In the bibliography module, a group mixing a cross-reference key with a citation key is now invalid instead of text, locators contain a no-break space, an English numeric edition reads "2nd ed." instead of "2 ed.", `.bib` errors carry `Source::Bibliography`, and a missing key names its item instead of a byte range. Layout test helpers pass the formatted citations.
- Visual review: rendered from `/tmp/druck09/final` with `druck render <repo>/samples/report.md -o report.pdf`, the same with `--theme <repo>/samples/themes/report.json` and with `--set citation-style=numeric`, and `samples/report-de.md` in both styles, rasterized with `pdftoppm -r 80 -png`, and looked at all 50 pages (pages that compared pixel-identical across styles once). Citations sit in prose, in the two-column section, in footnotes, at a column break ("Schäfer (2018, / S. 40–52)"), and after the long table at a page end. The English bibliography starts under the conclusion and continues on the next page with "References" in the running header; the German one fills two balanced columns after a `::: bibliography` inside `columns`. Author-date entries are sorted by name with "Knuth 1984a" and "1984b"; numeric ones follow first citation, and "[@tschichold1928; @bringhurst2004]" reads "[1, 3]". German entries show "Hrsg.", "3. Aufl.", "Masterarbeit", and "Abgerufen am". Two entries break badly, see the first follow-up.
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

- Unquoted decimals in `meta` and `author` lose trailing zeros (`1.50` reads `1.5`); serde-saphyr keeps source text only for plain string fields.
- A URL without a single slash that is wider than its column is still an error.
- `{pages}` and other page placeholders are rejected in the body; supporting them would need the settling loop to carry the page count.
- Reading speed for `{reading-time}` is fixed per language, and `{build-date}` uses UTC, not the local time zone.
- Font bytes from disk are leaked once per render. Fine for the CLI; the phase 2 crate must own them (see decisions).

## Updating this file

Replace the resume note with the latest completed work and next action. Record exact verification commands and results, remaining criteria, and blockers. Check a milestone only after its completion gate passes. Preserve useful follow-ups without copying the product requirements or decision history here.
