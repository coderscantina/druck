# Implementation decisions

The product choices are authoritative in [the phase 1 briefing](BRIEFING.md) and [the phase 2 briefing](RUST_CRATE_API_PHASE_2.md). This file records implementation choices made within those contracts.

## 2026-10-06: Milestone 01 foundation

### Project layout

One binary crate. `src/main.rs` owns argument parsing, file reading, diagnostic printing, and exit codes. `src/config/` owns configuration and performs no filesystem access: it takes parsed values and absolute source paths. The resource existence check runs in the CLI. This keeps phase 2 free to supply its own resolver. Nothing is public API.

### Dependencies

- `clap` (derive) for the CLI.
- `serde`, `serde_json`, and `serde_path_to_error`, which gives the property path of a typed deserialization error.
- `serde-saphyr` for YAML front matter. `serde_yaml` is deprecated. serde-saphyr is pure Rust, follows YAML 1.2 (`no` stays a string), and reports line and column.
- `jsonschema` as a dev-dependency only, with default features off to avoid HTTP and TLS crates, for the schema agreement tests.

Licenses checked on 2026-10-06 with `cargo metadata`: every runtime crate is MIT, Apache-2.0, BSD-3-Clause, or Unicode-3.0.

### Theme format and validation

Themes are JSON with kebab-case keys and `"version": 1`. [The schema](../schema/theme.v1.schema.json) is written by hand so it can carry author-facing descriptions. [Schema tests](../src/config/schema_tests.rs) keep it in agreement with [the types](../src/config/theme.rs): the default theme must validate, both must declare the same fields, and a table of partial themes must be accepted or rejected by both.

The Rust types describe a complete theme. A partial layer is validated by merging it onto the bundled default and deserializing the result, so one set of types serves complete and partial themes. Front matter and `--set` are typed separately and then mapped onto theme paths. The merged configuration is deserialized and checked again for references and combinations.

The schema does not require `text` in header and footer slots, because a partial theme may change only `required` on an inherited slot. The typed check catches a slot without text.

### Null

`null` is accepted only where it removes something: page variants (fallback applies), `header` and `footer` of a variant (none), band slots (empty), and the italic, bold, and bold-italic font faces. Optional title slot fields may be omitted but not set to `null`. Front matter rejects `null` except for faces in `font-files`, which use the theme font type.

### Tokens and measurements

Four token groups: `fonts`, `sizes`, `spacing`, `colors`. A field accepts a literal or `$group.name`, and the group must match the field kind. Tokens may reference tokens of the same group; undefined references and cycles are errors.

Lengths are strings with a unit: `pt`, `mm`, `cm`, `in`, or `em`. Absolute units become points when parsed. Negative lengths are rejected. Line height is a unitless number from 0.8 to 3. The `em` basis depends on context and is documented in [themes](THEMES.md): the body size for font sizes and page geometry, the element's own size for its other lengths. The body size must be absolute.

### Document settings and overrides

Front matter and `--set KEY=VALUE` share one type ([front_matter.rs](../src/config/front_matter.rs)). Settings write fixed theme paths: `document.*`, `page.*`, `styles.body.*`, and the font tokens `body`, `heading`, and `mono`. Themes should use those tokens so the font settings take effect. Neither input can reach templates. `--theme` replaces the front matter `theme`.

### Resource origins

[Merging](../src/config/merge.rs) records which layer supplied each value. A resource path keeps the origin of that layer: bundled, theme directory, document directory, or working directory for CLI values. Paths are never rebased. URLs are rejected. `check` reports missing files; bundled paths are not checked until bundled assets exist.

### Default fonts

The default theme names Libertinus Serif and Libertinus Mono (SIL OFL 1.1) as bundled resources. Milestone 02 added the files, see below.

### Templates and page variants

Slot text uses `{title}`, `{subtitle}`, `{author}`, `{date}`, `{abstract}`, `{section}`, `{subsection}`, and `{page}`, with `{{` and `}}` for braces. Values are inserted as text. Page-dependent placeholders are rejected in title slots, and `{abstract}` in headers and footers.

Each page uses the first variant present in its chain. Title page: `title`, `body`. First body page: `first`, `odd` or `even`, `body`. Other pages: `odd` or `even`, `body`. Parity follows the physical page index in the PDF, counting from 1 and including the title page, so odd pages are right-hand pages in duplex print. Odd pages put the inner margin on the left.

Open for milestone 08: the displayed page-number sequence, which heading supplies `{section}` on a page, and when required slots are checked.

### Layout directives

Fenced containers in the style of Pandoc divs, documented in [authoring](AUTHORING.md): `::: columns`, `::: full-width` (only directly inside `columns`), and `::: keep`, each closed by a line of colons. `::: page-break` stands alone. Directive lines inside code blocks are code. [Milestone 04](#2026-10-07-milestone-04-pagination-and-footnotes) added the parser. Caption, label, cross-reference, and citation syntax is decided in milestones 06, 08, and 09.

### CLI

`kyber check <doc>` validates and can `--print-config`; `kyber render <doc>` fails until milestone 02. Exit code 0 is success, 1 means diagnostics were reported, and 2 is a usage error.

## 2026-10-06: Milestone 02 rendering path

### Pipeline

`main.rs` reads the file and calls four internal stages: [markdown](../src/markdown.rs) parses the body into [the document model](../src/document.rs), [text](../src/text/mod.rs) loads fonts and shapes, [layout](../src/layout/mod.rs) produces [positioned pages](../src/page.rs), and [pdf](../src/pdf.rs) writes them. Layout never reads files and PDF output never makes layout decisions, which keeps both usable by the phase 2 crate. The stages were built in parallel against these typed interfaces.

### Dependencies

- `pulldown-cmark` with default features off, for CommonMark with byte offsets. Tables, footnotes, strikethrough, and task lists are enabled only so they can be recognized and reported. Math is left off so `$` in prose stays text.
- `krilla` for PDF output. It embeds and subsets fonts, writes ToUnicode maps from the glyph-to-text ranges we pass, and supports link annotations and, later, outlines and images. Its coordinates match ours: points from the top-left, y down.
- `rustybuzz` 0.20 for shaping, the same version krilla already depends on, so there is one shaping engine and one font parser in the build.

Licenses checked on 2026-10-06 with `cargo metadata`: all runtime crates are permissive (MIT, Apache-2.0, BSD-2/3-Clause, Zlib, Unicode-3.0, or alternatives offering one of these). Release notices are a milestone 10 task.

### Fonts

Libertinus v7.051 from the official release, SIL OFL 1.1, in [fonts/](../fonts) with [the license](../fonts/OFL.txt). Serif and Mono regular cover German and English punctuation (`äöüÄÖÜß „“ ‚‘ – — ’ “ ” …`), checked by shaping without missing glyphs. Bundled files are compiled in with `include_bytes!`; the default theme refers to them by path with the bundled origin.

Font bytes read from disk are leaked once so the shaper and krilla share one `'static` copy. That is fine for a one-shot CLI. The phase 2 crate renders repeatedly in one process and must own the bytes instead, for example with `Arc` and a face parsed per use or a self-owning face.

Shaping uses `kern` and `liga`, left-to-right, with the document language. Inline emphasis and strong text fall back to the closest available face (documented in [themes](THEMES.md)); block styles still require their face at validation. A glyph id 0 (no glyph) is an error naming the character.

### Unsupported content

Every Markdown construct that does not render yet is an error with its location, and `render` writes nothing. All such errors are reported in one run. Rendering a document while dropping content would hide the loss, which the briefing forbids. A paragraph starting with `:::` is reported as an unsupported layout directive until the directive parser arrives.

### Temporary layout

Line filling was greedy, word by word, with no hyphenation; [milestone 03](#2026-10-06-milestone-03-paragraph-composition) replaced it. [Milestone 04](#2026-10-07-milestone-04-pagination-and-footnotes) replaced first-fit page breaks.

Settled layout rules that later milestones keep:

- `indent` narrows a block on both sides. Quotations nest their indent.
- `first-line-indent` applies only to a paragraph that follows a paragraph.
- Adjacent vertical spaces collapse to the larger; space at a page top is dropped.
- Lines in a block are `size × line-height` apart. The baseline sits where the font's ascender and descender are centered in that height.
- Emphasis toggles italic, so it is upright in italic text. Strong sets bold.
- List markers end half an em before the item text. Bullets repeat the last entry at deeper levels; numbered lists keep their start number.
- Code lines are never wrapped. Tabs expand to four spaces. A code line wider than the text area is an error at its source line.

### PDF and CLI

The PDF has no creation date, so repeated renders are byte-identical. Title, authors, and language go into the document metadata. `kyber render <doc> [-o PATH]` writes next to the document with a `.pdf` extension by default; `-o` is relative to the working directory.

## 2026-10-06: Milestone 03 paragraph composition

### Line breaking

[Breaking](../src/layout/paragraph/breaking.rs) follows Knuth and Plass: words are boxes, spaces are glue, hyphenation points and explicit hyphens are penalties, and the breaks with the least total demerits over the paragraph win. Widths are the shaped widths of milestone 02. Words are shaped once; a word split across lines is reshaped only at the break, so ligatures and kerning stay correct on both sides.

Parameters, all internal constants for now:

- Interword space stretches by 1/2 and shrinks by 1/3 of its natural width, in justified text only.
- Badness is `100 × ratio³`, capped at 10 000. A first pass accepts lines up to badness 200. If no solution exists, a second pass accepts any line that is not overfull. A paragraph fails only if a single unbreakable piece is wider than its line.
- Demerits per line are `(10 + badness)² + cost²`. A hyphen or explicit-hyphen break costs 50. Two hyphenated lines in a row add 10 000, a hyphen before the last line adds 5 000, and adjacent lines more than one spacing class apart (tight, decent, loose, very loose) add 10 000.
- Ragged text (`left`, `center`, `right`) keeps natural spaces and is scored against a soft edge of 3 em, so lines are evened out without stretching.
- Active breaks drop out as soon as a line from them would be overfull, so work grows with paragraph length times the breaks within one line, not quadratically. Ties go to the earlier candidate. Output is deterministic.

Lines ending at a hard break or the paragraph end keep natural spacing. Spacing only changes between words; glyph widths are never adjusted.

### Hyphenation

`hypher` 0.1.8 (MIT or Apache-2.0) embeds the TeX patterns as compiled tries, about 230 KB for the two languages, so rendering needs no files and no system data. Only the `english` and `german` features are built, plus `alloc` so words over 45 bytes cannot panic. Pattern sources and licenses, checked on 2026-10-06 in the hypher repository:

- `hyph-en-us`, American English, Gerard D. C. Kuiken, 1990 to 2005. Copying and distribution with or without modification are permitted provided the notice is preserved.
- `hyph-de-1996`, German reformed orthography, Deutschsprachige Trennmustermannschaft, version 2024-02-28, MIT.

[Rules](../src/layout/paragraph/hyphenation.rs): `document.lang` selects the patterns, and only blocks whose style has `hyphenate: true` get hyphenation points. Fragment limits are the patterns' own (English two letters before and three after, German two and two). Only runs of letters are hyphenated, so punctuation and digits stay whole. Runs with a capital after their first letter (acronyms, camel case), words that look like URLs or e-mail addresses, inline code, and link text that spells out its URL are never hyphenated. Hyphenation works across style changes inside a word, and the hyphen takes the style of the text before it.

Words may also break after a hyphen or em dash that joins two words ("e-mail", "CommonMark-Spezifikation"), in every style except code. That adds no character.

The drawn hyphen is part of its run's text, so extracted text reads "hyphen-" at the line end, as with other typesetters. Readers such as `pdftotext` rejoin hyphenated words; they also drop an explicit hyphen at a line end, which we cannot prevent without marked content.

### Optical margin alignment

In justified text, the [first and last glyph](../src/layout/paragraph/protrusion.rs) of a line hang into the margins by a share of their advance. Breaking accounts for the hang, so a protruding line is still exactly justified against the margin. Quotes have one value for both sides, because „ “ ‚ ‘ open or close depending on the language.

| Characters | Share of advance |
| --- | --- |
| `.` `,` hyphen `'` `‘` `’` `‚` `‛` | 0.7 |
| `:` `;` `"` `“` `”` `„` `‟` | 0.5 |
| `–` `«` `»` `‹` `›` | 0.3 |
| `!` `?` `—` | 0.2 |
| Everything else | 0 |

Only one glyph per edge protrudes; a comma after a closing quote hangs, the quote does not.

### Consequences

- The breaking constants are not theme settings. A theme can only switch `align` and `hyphenate` per block. Expose them only if review asks for it.
- German compounds break at any pattern point, not preferably at compound boundaries ("Donaudampfschifffahrtsge-sellschaft"). Weighting compound boundaries needs data the patterns lack.
- The breaker takes one line width for the first line and one for the rest. That was enough for [milestone 05](#2026-10-07-milestone-05-column-layouts): a paragraph never changes column width midway, because it stays in one column section. Later shapes may need a width per line, which the node structure allows.

## 2026-10-07: Milestone 04 pagination and footnotes

### Directive parsing

[The parser](../src/markdown.rs) finds directive lines before CommonMark sees the text: every line that starts with `:::` in its first column, outside the code blocks of a first parse. It overwrites them with spaces, so byte offsets and locations stay valid and CommonMark sees a blank line, and applies them between the top-level blocks they stand between. Authors need no blank lines around directives, and a closing `:::` right after a paragraph line is not swallowed by the paragraph.

Errors, each at the directive's line: an unknown name, a fence never closed, a closing line with nothing open, `page-break` inside `keep`, nested `columns`, `full-width` not directly inside `columns`, a directive line inside a block's text (for example between the items of a loose list), and a `:::` paragraph inside a list, quotation, or footnote. An unknown name still opens a container so its closing line matches, and it is not reported again as unclosed.

`columns` and `full-width` become `Block::Columns` and `Block::FullWidth`. Layout reports them as not supported yet, so milestone 05 only adds layout. A keep group is transparent for first-line indents. Page breaks in a row give one break, and a page break before all content or after it has no effect, so no empty pages appear.

### Footnotes

Syntax is the GitHub form that pulldown-cmark parses with `ENABLE_FOOTNOTES`: `[^label]` references and `[^label]:` definitions anywhere, continued by four-space indents. Labels match without regard to case. Notes are numbered by reference order, which is their index in `Document::footnotes`.

All of these are errors with the location, because each would otherwise drop content or make placement ambiguous:

- A reference without a definition. pulldown-cmark leaves it as literal text, so the parser looks for `[^label]` text that does not start with an escape.
- A definition never referenced. It would have no page to go on.
- A second reference to the same note. A note starts on the page of its reference, and two references would make that ambiguous.
- Two definitions of one label, and a reference inside a footnote.

The marker is the number in the regular face of the block style at `inline.footnote-marker.size`, raised by `raise`, and part of the preceding word, so no line break separates them. A note starts with the same raised number and a space, in `styles.footnote`. The default theme now gives that style a first-line indent, so the paragraphs of a long note are distinguishable.

The note area spans the text width at the foot of the page: at least `footnotes.gap` below the body text, the separator, then each note after `footnotes.spacing`. Notes keep their order, so only the last note on a page can continue. It breaks between two of its lines. On the next page the rest follows a line with its raised number and the `continued` label in italics, "(continued)" or "(Fortsetzung)".

### Page breaks

[The composer](../src/layout/pages.rs) receives the body as lines, each with the collapsed space above it and a rule for ending a page after it: allowed with a cost, never, or forced. Headings never end a page. Lines inside a keep group never end one, except the group's last line. A page break forces the end.

A page costs the penalty of its break, plus a fill cost when it is not the last page and does not end at a forced break. Space between blocks may stretch by up to half its natural height. The fill cost is `100 × r³` for the share `r` of that stretch used, plus `1000 × s²` where `s` is the remaining shortfall in body lines. Break penalties:

| Break | Cost |
| --- | --- |
| Between blocks | 0 |
| Between two lines of a block | 50 |
| After a block's first line (orphan) | 5 000 more |
| Before a block's last line (widow) | 5 000 more |
| Continuing a footnote on the next page | 2 000 |
| A page holding only continued footnotes | 10 000 |

These rules apply to paragraphs, headings, list items, quotations, and code blocks alike.

The search is dynamic programming over the places a page may end. A state is a break position together with how many footnote lines have been placed, because carried note text changes what fits next. From each state it extends one page at a time until the body lines alone exceed the text height, so the work is proportional to lines times lines per page, times the few note states per position. On each candidate page it places as many note lines as fit, at least through the first line of every note referenced on the page. Ties go to the earlier state, and nothing depends on hashing or timing, so breaks are deterministic.

Before the search, every run of lines that cannot be broken is checked against the text height together with the first lines of its notes. A run that is too tall is an error at the keep group's directive, or at the heading that starts it. A footnote line too tall for a page of its own is also an error. With those checks the search always finds a solution; if it ever did not, layout reports it rather than writing a partial PDF.

The constants are internal. Visual review of the samples set widow and orphan costs to 5 000; at 3 000, a widow was preferred over running a page two lines short.

### Regions

A planned page is a list of body regions stacked from the top plus one range of footnote lines across the full width. [Milestone 05](#2026-10-07-milestone-05-column-layouts) added column regions.

### Consequences

- Pages end shorter to avoid widows, orphans, and stranded headings. The repeated English sample went from 99 to 104 pages.
- Notes have no widow or orphan rules. A continued note may leave a single line on either page.
- A line hyphenated at the end of a page costs nothing extra.
- Lists and code inside notes use their own styles at body size.
- The scoring constants are not theme settings.

## 2026-10-07: Milestone 05 column layouts

### Region model

[The flow](../src/layout/mod.rs) stays one sequence of lines. Lines of a `columns` section are set at the column width, `(text width - page.column-gap) / 2`, and their line ranges are recorded as column runs. A `full-width` block ends one run, is set across the text area, and starts the next, so it behaves like closing and reopening the section. Each layout change requests at least `page.column-gap` of space, collapsing with block spaces like any other, and starts a new paragraph sequence without a first-line indent.

[The composer](../src/layout/pages.rs) splits a candidate page's lines into regions: full-width runs and the part of a column run on that page. A column region holds its lines in reading order, the first column above the split and the second below. The next region starts below the taller column. Page breaks, footnotes, and break costs work on the same line sequence as before, so a layout change never forces a page by itself and the existing samples render byte for byte as in milestone 04.

### Balancing

Every column region is balanced, whether the section ends on the page or continues. A split may follow any line a page may end after, at that line's break cost; the second column may also stay empty. The taller column is lowest where the two column heights cross, found by binary search over prefix sums. The search then walks outward in both directions while being less even could still pay off. A split costs its break cost plus 3 000 × (excess)², where the excess is how many body lines the region is taller than its most even allowed split. So a region accepts one uneven line to avoid an orphan or widow (5 000), but not two. Ties go to the taller first column.

A region continuing to the next page is balanced too, and the search picks the page end where its balanced height reaches the bottom, so both columns run full. When keep groups or headings force an uneven split there, the shorter column's shortfall counts as a short page in the fill cost, so the search prefers a page end that fills both columns.

Each column's spaces may stretch within the usual bound (half their natural height) toward the region's height, so column bottoms line up when the spaces allow. A column that cannot reach it stays at natural spacing; columns never stretch beyond the bound to balance. A region can grow with the page's stretch by as much as both columns can follow.

Work per candidate page end is a binary search and a few steps, so pagination stays proportional to lines times lines per page. A 91-page column input renders in 0.46 s against 0.39 s for 104 single-column pages; most of the difference is setting more, shorter lines.

### Footnotes and keeps

Footnotes keep one full-width area at the foot of each page. They are numbered in reading order, which runs down the first column and then the second, and the page's note area reduces the room every region on it can use. All other note rules are unchanged.

Headings and keep groups mark their lines as never ending a page, and the same marks forbid a column split. A keep group inside columns therefore stays in one column. A run of lines that cannot break, checked against the text height as before, is also checked against a column, since a column is as tall as the text area.

### Page breaks and nesting

`page-break` inside a column section ends the page. The columns above it are balanced like any region, and the section continues on the next page. A column break directive is not part of the briefing and was not added.

`columns` inside `keep` is now a parser error. Keeping a whole section on one page would need a third break rule, no page break but a column break allowed, for a case authors can express by keeping the parts inside the columns instead.

### Diagnostics

A keep group, or a heading with the start of its text, taller than a column is reported at its directive or heading, as on pages. A word or code line wider than a column is reported at its line with the column width. Nothing is clipped, dropped, or spaced out to fit.

### Consequences for milestones 06 and 07

Images and tables inside a column section get the column frame, so they scale to the column width; inside `full-width` they get the text width. If they enter the flow as lines, with an image and its caption marked as unbreakable, balancing and pagination apply unchanged: a unit that does not fit in the rest of a column moves to the next column or page through the same split and break costs. Table rows that never split work the same way. Downscaling to the column height is the 06 and 07 counterpart of the keep diagnostic.

## Recording a decision

Add a short dated entry when a choice affects future work. State the choice, reason, affected interface or behavior, and any unresolved consequence. Link to code, schema, or tests once they exist. Replace superseded guidance with a reference to the newer decision.

Keep dependency versions and runnable commands in their actual manifests and tooling. Do not copy them here. Record why a dependency was chosen when that reason is not evident from the code.
