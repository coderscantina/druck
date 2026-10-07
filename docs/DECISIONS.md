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

[Milestone 08](#2026-10-07-milestone-08-document-structures-and-templates) decided the displayed page-number sequence, which heading supplies `{section}` on a page, and when required slots are checked.

### Layout directives

Fenced containers in the style of Pandoc divs, documented in [authoring](AUTHORING.md): `::: columns`, `::: full-width` (only directly inside `columns`), and `::: keep`, each closed by a line of colons. `::: page-break` stands alone. Directive lines inside code blocks are code. [Milestone 04](#2026-10-07-milestone-04-pagination-and-footnotes) added the parser. Caption syntax was decided in [milestone 06](#2026-10-07-milestone-06-images-and-captions) for figures and [milestone 07](#2026-10-07-milestone-07-multipage-tables) for tables; label and cross-reference syntax in [milestone 08](#labels-and-cross-references); citation syntax in the [milestone 09 groundwork](#citation-syntax) and [milestone 09](#citations-in-the-document).

### CLI

`kyber check <doc>` validates and can `--print-config`; `kyber render <doc>` fails until milestone 02. Exit code 0 is success, 1 means diagnostics were reported, and 2 is a usage error.

## 2026-10-06: Milestone 02 rendering path

### Pipeline

`main.rs` reads the file and calls four internal stages: [markdown](../src/markdown.rs) parses the body into [the document model](../src/document.rs), [text](../src/text/mod.rs) loads fonts and shapes, [layout](../src/layout/mod.rs) produces [positioned pages](../src/page.rs), and [pdf](../src/pdf.rs) writes them. Layout never reads files and PDF output never makes layout decisions, which keeps both usable by the phase 2 crate. The stages were built in parallel against these typed interfaces.

### Dependencies

- `pulldown-cmark` with default features off, for CommonMark with byte offsets. Tables, footnotes, strikethrough, and task lists are enabled only so they can be recognized and reported. Math is left off so `$` in prose stays text. Footnotes render since [milestone 04](#2026-10-07-milestone-04-pagination-and-footnotes) and tables since [milestone 07](#2026-10-07-milestone-07-multipage-tables).
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

## 2026-10-07: Milestone 06 images and captions

### Dependencies

- `krilla-svg` 0.8.1, the SVG companion of krilla 0.8, draws a `usvg` tree into the PDF as vectors and text. It brings `usvg` and `resvg` 0.47, `fontdb`, `tiny-skia`, and their parsers.
- `usvg` 0.47 directly, with default features off and only `text`, so there is no system font discovery or memory mapping.
- `png` 0.18 and `zune-jpeg` 0.5, the versions krilla already uses for its `raster-images` feature, to validate raster images.

Licenses checked on 2026-10-07 with `cargo metadata`: all 19 new crates are MIT, Apache-2.0, BSD-2/3-Clause, or Zlib, or offer one of these.

### Loading and validation

[The parser](../src/markdown.rs) collects the distinct image paths in order of first use. The CLI reads each file once, relative to the document's directory, and [decodes](../src/image.rs) it. Layout receives the decoded images and never reads files, as before. The bytes are shared between all references, and krilla writes each image into the PDF once.

The format comes from the content: magic bytes for PNG, JPEG, GIF, and WebP, and a successful parse for SVG. The extension must agree, so a JPEG named `.png` is an error. GIF and WebP are named as unsupported; krilla could embed them, but the briefing lists PNG, JPEG, and SVG. krilla reads only the headers of raster images and embeds JPEG data as is, so a truncated file would otherwise produce a broken PDF. Raster images are therefore decoded fully when loaded. krilla decodes PNG again while writing; that double decode is cheap at document image sizes, and avoiding it would mean giving krilla raw pixels and losing its PNG handling.

SVG is parsed once with only the bundled Libertinus fonts in its font database, serif and default family Libertinus Serif, monospace Libertinus Mono. An SVG that refers to another file is an error rather than drawn without that part. Embedded data URLs work.

URLs, including `data:` URLs, in Markdown are rejected by the parser, as in configuration. Theme `images` keep their theme origin through resolution as before; title slots draw them since milestone 08, decoded by the CLI like document images.

### Caption syntax

An image alone in its paragraph is a figure, and its description is the caption, as in Pandoc's implicit figures. The description keeps emphasis, strong text, and code. This needs no new syntax and leaves room for milestone 08: an attribute such as `![Caption](file.png){#fig:x}` currently makes the paragraph hold text after the image, which is an error, so 08 can give it a meaning without breaking documents. [Milestone 08](#labels-and-cross-references) made it the figure label.

Errors: text before or after the image in its paragraph, an image in a heading, link, or footnote, an empty path, and an image title. A title would otherwise be dropped. CommonMark leaves an image it cannot parse as text, starting with a separate `![` event; that is reported too, so a path with spaces or a footnote in a description never prints as literal Markdown. As a consequence, captions cannot hold footnote references.

A captioned image is numbered in document order and its caption starts with the `figure` label, the number, and `captions.separator`, all in the caption style. An image with an empty description has no caption and takes no number. Since [milestone 08](#numbering) the number comes from a pass over the document model before layout.

### Size and placement

Raster images count one pixel as one point. PNG density chunks and JPEG density fields are ignored, so a 144 dpi screenshot appears at twice its intended size unless it is scaled down to fit, which most screenshots are. SVG uses CSS units, 0.75 pt per pixel.

An image keeps its proportions and shrinks to the smaller of two bounds: the width of its frame (text area, column, or the narrower frame of a list or quotation) and the text height minus its caption, the space between them, and the lines it must stay with above it, such as a heading. It never grows, because enlarging raster images blurs them and there is no setting that asks for it. It is centered in its frame. A caption that leaves no height at all is an error at the image.

An image enters the flow as one line as tall as the image. If it has a caption, that line and every caption line but the last never end a page or column, so the composer treats image and caption as a keep group: pagination, column splits, balancing, and footnote placement apply unchanged, and an image that does not fit moves to the next column or page with its caption. The figure is spaced by the caption style: its `space-after` above and below the figure, its `space-before` between image and caption. No new theme settings were needed.

Scoring is unchanged. Without floats, an image that does not fit leaves a short page, and the square fill cost can then prefer a widow over an even shorter page, as on page 1 of `samples/images.md`. Capping the fill cost was tried and rejected: when every short page costs the same, the search ends pages early around images.

### Consequences for milestones 07 and 08

- Tables can enter the flow the same way, with rows as lines that never split. A table caption can reuse the caption path with the `table` label and its own counter.
- Cross-references need figure labels and numbers before layout. [Milestone 08](#labels-and-cross-references) added `{#fig:x}` and moved numbering to a pass before layout.
- Title slot images can use the same loader with the theme origin.

## 2026-10-07: Milestone 07 multipage tables

### Table model and syntax

Tables are GitHub pipe tables, parsed by pulldown-cmark with `ENABLE_TABLES`. [The parser](../src/markdown.rs) turns one into `Block::Table` in [the document model](../src/document.rs): the column alignments from the delimiter row (`None` keeps the cell style's alignment), a header row, and body rows, each cell with its location and flat inline content like a paragraph. Every row has one cell per column. A short row is padded with empty cells, as GitHub does. A row with more cells than the header is an error, because pulldown-cmark drops the extra cells without an event; the parser counts unescaped pipes in the row's source to find them. Merged cells are out of scope.

Tables may stand wherever blocks may: in lists, quotations, columns, `full-width`, and `keep`. A table in a footnote is an error, since the note area has no room for that layout. Images in cells are errors. Footnote references in body cells work and are numbered in reading order. A reference in a header cell is an error, because the header repeats on every page and the note would have no single page.

### Caption syntax

A paragraph directly after the table that starts with a colon and a space is its caption, as in Pandoc: `: Caption text`. A blank line must separate them, since GitHub tables run until a blank line. The caption keeps emphasis, strong text, and code. These are errors at their location:

- A `: ` paragraph that does not directly follow a table, or a second one.
- A body row whose only content is a first cell starting with `: `, which is a caption written without the blank line.
- A footnote in a caption, as for figures.
- A caption ending in `{#...}`, reserved until [milestone 08](#labels-and-cross-references) made `{#tbl:x}` the table label.

A captioned table is numbered in document order with the `table` label and its own counter, apart from figures, and the caption is set above the table across the frame width, in the caption style. A table without a caption takes no number. Since [milestone 08](#numbering) the number comes from a pass before layout, like figure numbers.

### Column widths

[Table layout](../src/layout/table.rs) shapes every cell once. [Paragraph composition](../src/layout/paragraph.rs) now has a prepare step that shapes and builds the breaking items, and a set step that breaks at a given width, so a cell is measured and set from one shaping. Two widths per cell:

- Natural: the widest line when only hard breaks end lines.
- Minimum: the widest unbreakable piece, after hyphenation if the cell style hyphenates, with the added hyphen.

A column's widths are the largest of its cells. Cell padding is added on both sides of each column. If the natural widths fit the frame, every column keeps its natural width and the table is centered. Otherwise each column gets one common width clamped between its own minimum and natural width, with the common width chosen so the columns fill the frame. A column of short entries stays on one line, and long text columns share the rest equally. The common width is found by bisection, 64 steps, which is deterministic and exact enough. If even the minimum widths do not fit, the error names the cell with the widest word, its width, and the table's minimum width.

The frame is the text width, the column width inside `columns`, the text width inside `full-width`, and the narrower frame of a list or quotation. Cells are set with the paragraph breaker at their column width plus 10⁻⁶ pt, so text measured to fit exactly is not broken by rounding.

### Rows in the flow

A table enters the flow as lines. A row is one line, as tall as its tallest cell plus the padding above and below and a rule below it; the header row also has a rule above. Cells are top aligned. The caption lines and the header row never end a page or column, so the caption stays with the header and the first row. Breaks between body rows follow the paragraph costs: 50 between rows, 5 000 more for leaving the first row alone at the bottom or the last row alone at the top. The table is spaced like a figure: the caption style's `space-after` above and below it and `space-before` between caption and table. No new theme settings were needed.

Because a row is a single line, pages and columns can never break inside it. Pagination, column splits, balancing, keep groups, and footnote placement apply unchanged.

### Header repetition at the composer boundary

The composer receives each table's line range, the range of rows a page or column may start at, and a copy of the header row ([`pages::Table`](../src/layout/pages.rs)). From it the composer builds one entry per body line: the header to set above that line when a page or column starts there. Three places read it:

- A page starting inside a full-width table counts the header in its height.
- A column, first or second, that starts inside a table counts it in the column height used for balancing.
- Rendering places the header copy at the top of that page or column.

The lookup is constant time, so pagination stays proportional to lines times lines per page. The binary search for the even column split now sees a header jump in the second column's height; the walk outward from the crossing point covers that.

### Diagnostics

- A row taller than the text area together with the repeated header and the first lines of its notes is reported at the row: "this table row with the repeated header is ... high". A column is as tall as the text area, so the same check covers columns.
- The caption, header, and first row together too tall are reported at the table.
- A word too wide for the minimum layout is reported at its cell, see above. Characters without a glyph are reported at their cell.
- All parser errors above carry their location.

### Composer gap in the images sample

Page 2 of `samples/images.md` ends about a quarter page early. The text after the chart does fit: the page ending after that paragraph and its note is a valid candidate, and cheaper for page 2 alone (fill cost 92 191 against 113 041). The search still prefers the earlier end, because it leaves page 3 fuller: 104 387 against 163 181 for page 3. Both pages end short since the tall image on page 4 cannot move up, and the square fill cost spreads the shortfall over both pages rather than leaving it on one. This is the scoring trade-off recorded for milestone 06, not a feasibility defect. Tables do not trigger it, because rows are short units that the search can move one at a time. The code was left unchanged.

### Consequences for milestone 08

- Table numbers and the `: Caption {#tbl:x}` label: done in [milestone 08](#labels-and-cross-references), with numbering in a pass before layout.
- A table of tables, if wanted, can read the same captions.

## 2026-10-07: Milestone 08 document structures and templates

### Labels and cross-references

A label names a heading, a figure, or a table and always starts with the prefix of its kind:

- `# Heading {#sec:name}`, read with pulldown-cmark's heading attributes. Other attributes are errors. Since [milestone 11](#attribute-syntax) a heading may also take one `{.name}` class.
- `![Caption](file.png){#fig:name}` directly after the image, spaces allowed. A label on an image without a caption is an error, because it has no number to show.
- `: Caption {#tbl:name}` at the end of a table caption.

Names are ASCII letters, digits, `-`, and `_`. A label with the wrong prefix or another name is an error at the label, and so is a second definition, naming the line of the first.

A reference is `@sec:name` or `[@sec:name]`. It shows the theme label of its kind, a no-break space, and the number: "Section 2.1", "Figure 3", "Table 2". A reference to an unnumbered heading shows the heading's text. `[@sec:name, page]` shows the `page` label and the target's page: "page 7". Every reference links to its target. The theme gained the labels `section` and `page`.

References are read from the source text of each text event, not from the parsed text, so `\@fig:x` stays text, and an `@` after a letter or digit, as in an e-mail address, is no reference. They are errors in headings, whose text goes into the table of contents, bookmarks, and running headers, which must not depend on pages, and inside link text. A reference to an undefined label is an error at the reference.

The prefix is the boundary with citations. An `@key` or `[@key]` whose key has none of the three prefixes is left as text for milestone 09. A bracket that starts with a prefixed label but holds anything other than `]` or `, page]` after it is an error, so milestone 09 is free to define groups and locators for citations without old documents meaning something else.

### Numbering

[A pass over the document model](../src/layout/structure.rs) before layout numbers headings, figures, and tables, gives each an anchor, and records the text every label is referenced by. Layout visits the same blocks in the same order and takes their entries in turn. Numbers do not depend on pages, so they are final before the first layout pass.

Headings are numbered when `numbered-headings` is on and their level is at most `numbering-depth`: "1", "1.1", "1.1.1". A heading resets the counters below its level. A level 2 heading before any level 1 heading is "0.1", as in LaTeX. The number and a space precede the heading text in the heading, the table of contents, bookmarks, and running headers. Headings inside footnotes are set unnumbered and are not part of the structure. Figures and tables keep their numbering from milestones 06 and 07, now from this pass.

### Title block and title page

With `document.title-page` the `title-page` slots fill the first page of their own. Otherwise the `title-block` slots start the body, but only if the document has a value for at least one of their placeholders, so a document without metadata gets no title block and no error.

An optional slot is omitted with its `space-before` when any of its placeholders lacks a value; blank values count as missing. Slots are spaced only by their `space-before`, so the result does not depend on which slots happen to be present. The title block ends with the new `title-block.space-after` (2em of the body size in the default theme), a schema addition. A slot in the `abstract` style starts with the `abstract` label in the `abstract-heading` style. Authors are joined with commas. Values are text: blank lines separate paragraphs, Markdown is not interpreted.

The title page stacks its slots from the top of the text area and keeps the first slot's space. Content taller than the text area is an error. The title block is one keep group at the top of the flow.

Required title slots are checked as soon as the configuration and metadata are known, by `kyber check` and by `kyber render`. The error names the document and the slot property.

### Page numbers, variants, and running headers

The displayed page number is the physical page number, counted from 1 and including the title page. It is the number a PDF viewer shows, and odd displayed numbers are odd physical pages, which milestone 01 chose for parity. There is no separate front matter numbering. As a consequence the length of the title page and the table of contents moves the numbers of body pages, which the settling below takes care of.

Variant selection follows milestone 01. With a title page the first body page is physical page 2, an even page, so the composer now takes the parity of its first page and puts the inner margin on the right there.

`{section}` is the first level 1 heading that starts on the page, otherwise the last one on an earlier page. `{subsection}` is the first level 2 heading that starts on the page after the section shown, otherwise the last level 2 heading before the page, unless a level 1 heading came after it. This follows LaTeX's right mark: the header describes what starts on the page. Both values include the number.

Header baselines sit `header-offset` above the text area, footer baselines `footer-offset` below it. Slots are aligned to the left edge, center, and right edge of the text area and set on one line. Slots that overlap or run past the text width are an error naming the band and page, since nothing is clipped. An optional slot without a value stays empty; a required one is an error naming the slot property and the page. Bands are drawn after the final layout pass, because they sit in the margins and never change the layout.

### Table of contents

With `toc` the `contents` label in the `toc-heading` style follows the title block, then one entry per heading up to `toc-depth`. Entries use `toc-entry`, are indented by `toc.level-indent` per level, and hang their later lines by one more level. The page number is right aligned in a column as wide as three digits, or the number if wider, so page numbers up to 999 do not change line breaks. With `toc.leader` dots at half-em steps on a grid shared by all entries lead to it, half an em from the text and the number. Each entry is a link to its heading. A `::: page-break` at the start of the body puts the body on a new page.

### Navigation

Layout places an anchor item at the top left of each heading's first line, each figure's image, and each numbered table's first caption line, which never repeats like the header row. [The output](../src/page.rs) carries the anchor positions and the heading outline. [PDF output](../src/pdf.rs) writes XYZ destinations for internal links and bookmarks with krilla 0.8, nesting bookmarks by level. URLs stay URI actions.

### Settling page numbers

The pages of the anchors the document shows (table of contents headings and targets of page references) are only known after layout. The first pass assumes page 1 for every anchor. After each pass the shown anchors' pages are compared with the assumed ones; if any differs, layout runs again with the pages it found. After 5 passes without agreement layout fails with the anchor that moved and its two pages, at the anchor's location. Nothing depends on timing or hashing, so the result is deterministic. A document without shown page numbers needs one pass; running headers and footers do not count.

Every pass lays out the whole document again, shaping included. Measured on 2026-10-07: the report sample and a 101-page report with 176 contents entries and 384 references both settle in 2 passes, the long one in 0.39 s. No damping was added. If documents that oscillate show up, reserving the widest page number seen so far for each reference would make widths grow monotonically and end the oscillation.

### Consequences for milestone 09

- Citations take the `@key` and `[@key]` forms that labels leave free. The branch in [the parser](../src/markdown.rs) where `reference_label` finds no prefix is where they go.
- A bibliography is generated content in the flow. Numeric citation labels do not depend on pages, but the bibliography's length moves later pages, so it must be laid out in every pass, like the table of contents.
- The milestone 08 integration scenario (`samples/report.md` in both themes) must be re-run with bibliography content.

## 2026-10-07: Milestone 09 citation groundwork

Built in parallel with milestone 08, as new files in [src/bibliography/](../src/bibliography/mod.rs). [Milestone 09](#2026-10-07-milestone-09-citations-and-bibliography) wired it into the parser and layout and removed its `allow` attribute. Required fields per entry type are in the module's doc comment and in [authoring](AUTHORING.md#entry-types-and-fields).

### Reader

A hand-written BibTeX reader, no dependency. Types and field names are case-insensitive. `@comment` and `@preamble` are skipped; `@string` macros and `crossref` are errors, as the briefing allows. Month macros (`jan`) are accepted; other bare macros are errors. Common LaTeX escapes for English and German text become Unicode (`\"a`, accents, `\ss`, `--`, `---`, `~`, escaped specials, protective braces). Any other escape is an error naming entry, field, and escape, but only in fields the entry type uses. `url` and `doi` stay verbatim. A syntax error stops reading; recoverable errors are collected together.

Only BibTeX fields are read: `year`, `journal`, `address`. BibLaTeX's `date`, `journaltitle`, and `location` are ignored like any unknown field.

### Citation syntax

`[@a]`, `[@a; @b]`, `[@a, p. 12]`, narrative `@a` and `@a [p. 12]`. Locators are `p.`, `pp.`, or `S.` with one page or a range; output is normalized per language ("pp. 3–5", "S. 3–5"). Narrative `@key` counts after the start of text, whitespace, or an opening bracket or quote, so `a@b.de` is not a citation but `ask @mike` is, and fails as a missing key. Keys with a `sec:`, `fig:`, or `tbl:` prefix are never citations. A bracket group that starts with `@` but cannot be read is returned as invalid so the integrator reports it.

Resolved in [milestone 09](#error-rules): a bracket group mixing a cross-reference label and a citation key is an error.

### Styles

Entries read the same in both styles: lead (authors inverted for the first, else editors, else a report's institution, else the title), year in parentheses, title (italic for book, thesis, report), type details, then DOI or URL as a link. Numeric adds an `[n]` label and orders by first citation; author-date sorts by lead with umlauts folded, then year, title, key, and disambiguates equal labels and years with a, b. Author-date citations read "(Smith 2024, p. 12)" and "Smith (2024)", with "and"/"und" for two authors and "et al." for three or more. Numeric citations read "[1, p. 12]", collapse runs to "[1–3, 5]", and narrative "Smith [1]". Given names print as written. Editions written as a number get the language's ordinal since [milestone 09](#formatting-changes): "2nd ed.", "2. Aufl.".

`finish` is a second step after collecting every citation, because year suffixes depend on the whole document.

### Diagnostics

Resolved in [milestone 09](#error-rules): `.bib` errors use `Source::Bibliography`.

## 2026-10-07: Milestone 09 citations and bibliography

### Citations in the document

The parser reads citations from the source text like cross-references and stores them in `Document::citations`: the parsed citation and the location of each key. Inline content holds `Inline::Citation`, an index into that list. A citation's text depends on every other citation (numbers, a and b suffixes), so [citations.rs](../src/citations.rs) formats them after parsing: it adds them to `Citations` in reading order, the body in order with a footnote's content at its reference, and calls `finish`. Layout replaces each citation with its text, as it does for cross-references, so the inline model stays flat.

A bracket group starts at a `[` text event whose source starts with `@` and ends at the first `]`; `syntax::find` reads exactly that slice. It may span lines, and soft breaks inside it are skipped. A narrative citation is read with `syntax::find` on the rest of its source line, so the citation syntax decides whether the `@` starts a word, and a locator in brackets must start on the same line. Citations in link text stay text, and citations in headings are errors, like cross-references, because heading text feeds the table of contents, bookmarks, and running headers.

### Placement and the heading

The bibliography goes at the end of the document. `::: bibliography` places it elsewhere; it stands alone like `page-break`, appears at most once, is not allowed in `keep`, and works inside `columns` and `full-width`. Without the directive the parser adds `Block::Bibliography` at the end of a document that cites, so structure and layout have one code path. A document without citations has no bibliography, even with the directive.

The heading is the theme's `references` label in the `heading-1` style, unnumbered. [Structure](../src/layout/structure.rs) records it as a level 1 heading without a number, so it is in the table of contents, the outline, and `{section}` without special cases, and the numbers of other headings do not change. This follows LaTeX's starred section with a contents line.

[Entries](../src/layout/bibliography.rs) are paragraphs in the `bibliography` style with a negative first-line indent of `hanging-indent`, as table of contents entries hang, at least `entry-spacing` apart, and break with the usual widow and orphan costs. A numeric entry starts with its label and a space. There is no separate label column; later lines hang by `hanging-indent` whatever the label width. Entries are laid out in every layout pass like other content, and add no shown page numbers, so they need no extra settling pass.

### Linking

Each entry's first line has an anchor. Every citation links to the entry of the first work it shows: the lowest number in the numeric style, the first work written in the author-date style. One link per citation keeps `Rendered` a string per citation; per-work links inside a group would need per-item output.

### Error rules

- A citation without a `bibliography` setting is an error at the first citation.
- A key that is not in the bibliography is an error at the key. `MissingKey` now carries the item's position instead of a byte range, which the parser has already turned into a location.
- A bracket group that starts like a citation but cannot be read is an error at the group. A group that mixes a citation key with a cross-reference label is an error in either order: `syntax` returns it as invalid when a citation key comes first, and the cross-reference parser rejects anything but `]` or `, page]` after a label.
- `Source::Bibliography(path)` identifies the `.bib` file. Its origin is the file's directory, like the document's. Reader errors use it with line and column. `Entry` and `Reference` carry the entry's position, so layout errors in an entry, such as a URL wider than a column or a missing glyph, are reported at the entry in the `.bib` file.
- A missing `.bib` file is reported by the existing resource check with the `bibliography` property. `render` reads the file once whenever it is configured, so a broken `.bib` fails even before anything cites it. `check` does not read it, as it does not parse the body.

### Formatting changes

- A locator joins its label and page with a no-break space: "p. 12".
- An edition written as a number gets the ordinal of the language: "2nd ed.", "2. Aufl.". Before, English printed "2 ed.", so a `.bib` file shared by English and German documents read badly in one of them.

### Measurements

Measured on 2026-10-07 on an Apple M1 Pro with 32 GB, see [progress](PROGRESS.md#verification). Documents with citations and a bibliography settle in the same passes as without: two with a table of contents, one without. The English sample text with 1 560 added citations sets 111 pages in 0.45 to 0.49 s, against 103 pages in 0.42 s without them.

### Consequences

- Bibliography entries expose a breaker limit: when no first line fits within tolerance, the second pass caps badness at 10 000, so all very loose first lines tie and the earliest break wins. An entry whose URL does not fit after the first line can then break after its first word. An uncapped second pass or an emergency stretch would fix it in [the breaker](../src/layout/paragraph/breaking.rs).
- URLs and DOIs never break, so long ones do not fit narrow columns. Allowing breaks after `/` in link text that spells its URL would lift that.

## 2026-10-07: Milestone 10 release groundwork

Built in parallel with milestone 09, without touching the renderer. The procedure is in [release](RELEASE.md).

- **Release profile:** fat LTO, one codegen unit, and stripped symbols. The macOS release binary went from 11.9 MB to 8.2 MB; build time from 30 s to 75 s. All sample PDFs were byte-identical between the two builds, so the profile does not change layout.
- **Notices:** [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) is generated by [scripts/notices.py](../scripts/notices.py) from `cargo metadata --locked` over the five release targets, normal dependencies only, with license texts from the registry sources. It covers the Libertinus fonts (OFL 1.1), the hyphenation patterns embedded by `hypher` (`hyph-en-us` with its notice-preserving license, `hyph-de-1996` MIT), and krilla's upstream license and notice, which its crates do not ship (copied to `scripts/license-overrides/krilla/`). The pattern headers come from the upstream `hypher` repository, not pinned to the 0.1.8 tag. CI fails when the file is stale.
- **Cross-platform layout check:** CI renders every `samples/*.md` (and `X-theme.pdf` when `samples/themes/X.json` exists) on each OS from an empty working directory with [scripts/render-samples.sh](../scripts/render-samples.sh), with `core.autocrlf false`, and [scripts/check-pdfs.sh](../scripts/check-pdfs.sh) compares the PDFs byte for byte. If byte equality proves too strict, compare `pdftotext -layout` output and page counts instead and record why.
- **Release builds:** [release.yml](../.github/workflows/release.yml) builds x86_64 and aarch64 Linux on Ubuntu 22.04 images (older glibc), aarch64 and x86_64 macOS, and x86_64 Windows on manual dispatch or `v*` tags, renders the samples with each runnable binary, and uploads archives with SHA-256 files. It does not create a GitHub release.
- **Not set:** `rust-version`, because no older toolchain was tested, and `license`, which waits for the owner's choice.

## 2026-10-07: Business documents amendment

Decided with the owner against a reference offer PDF: a cover with a metadata block, prose narrower than the tables, a three-column footer, and cost tables with detail lists in cells. The product contract is in [the amendment](BRIEFING.md#business-documents-amendment); milestones [11](milestones/11-typography.md), [12](milestones/12-page-geometry.md), and [13](milestones/13-rich-tables.md) deliver it.

- **Scope:** everything the reference needs, including eyebrows above headings, ✓ lists, Medium and Light weights, and totals rows. Totals are typed by the author, not computed.
- **Installed fonts:** a bare family name in `fonts` is looked up among installed fonts when it is neither bundled nor in `font-files`; `font-files` wins on a name clash. Absolute `font-files` paths already worked, but collections and weights did not. Lookup happens at the CLI boundary only, so the phase 2 crate takes a caller-supplied font resolver instead. A missing font is an error, since a silent fallback changes layout. Restricted embedding licenses only warn: the license is the user's to manage.
- **Weights:** numeric 100 to 900 with `regular` and `bold` as aliases, rather than one family per weight or a few named weights.
- **Custom styles:** named styles with `based-on` and overrides, applied with `{.name}`. Inferring styles from structure was rejected as too implicit. Directives could not serve, since they are not allowed inside lists and so not inside list-table cells. Placement: end of a heading or paragraph, a line before a list, start of a row or cell item.
- **Style properties:** `tracking` and `uppercase` for letter-spaced capitals, `keep-with-next` for eyebrows (a paragraph, so not in the outline), `bullets` on list styles for per-list markers, and `number-gap` for hanging step numbers typed by the author. A heading `eyebrow` attribute and automatic per-section counters were rejected as one-off machinery.
- **Geometry:** `margins.mirror: false` instead of separate `left` and `right` keys, which would be awkward to override across theme layers. `page.text-width` with a theme `wide` list of block kinds; authors do not mark wide blocks.
- **Anchored slot groups:** one model for the title page and the bands, anchored to the margin frame so a margin change moves them along. Absolute page coordinates stay excluded.
- **Metadata:** a free `meta` map in front matter used as `{meta.key}`; other unknown keys stay errors so typos are still caught. `{pages}` adds the page total.
- **Tables:** a `::: table` list table, because pipe tables cannot hold lists and grid tables are hard to edit. Column spans with an explicit `{span=n}`, because inferring spans from short rows would hide a missing cell. Alignment and widths as directive attributes. Header and row rules in the theme with a `rule-below: none` opt-out for totals.
- **Verification:** the offer theme with the commercial font stays outside the repository and is compared on the owner's machine. CI covers the features with synthetic samples and tests.
- **Milestones:** three, grouped as typography, geometry, and tables. 11 and 12 are independent; 13 needs both.

## 2026-10-07: Milestone 11 installed fonts and custom styles

### Installed fonts

- **Lookup:** `fontdb` 0.23, already in the tree through `usvg` and `krilla-svg`, is now a direct dependency, so no crate and no notice was added. [The CLI](../src/installed.rs) scans the system font directories, and fontconfig's on Linux, only when a style names a family that `fonts` does not define. Configuration records those families with the faces styles request (`installed_fonts`) instead of rejecting them, so `src/config/` and layout stay free of system access, and the phase 2 crate can supply its own resolver.
- **Matching:** a family name matches the typographic or legacy family name fontdb reads, ignoring ASCII case. Only the width closest to normal counts, so condensed faces do not stand in. Oblique counts as italic. Where two files hold the same face, italic beats oblique, then the first path and index win, so the choice does not depend on directory order. The whole family is loaded so emphasis and strong text find their faces; only faces used are embedded.
- **Errors:** a missing family or face is an error at the first style property that requests it, naming the family and face searched for and listing the installed faces. No PDF is written.
- **Restricted embedding:** a used face whose OS/2 `fsType` restricts embedding, forbids subsetting, or allows bitmaps only is embedded with a `warning:` line naming its file. Unused faces of the family do not warn.
- **Collections:** every face file carries its index in the resolved configuration, and font loading reads each file once, so the faces of one collection share its bytes.

### Weights and faces

- **Weights:** a number from 100 to 900 in steps of 100, with `regular` and `bold` for 400 and 700. Installed faces keep the weight class they declare, which may lie between steps; a style then matches them only through inline fallback, and the error for an exact request lists what is there.
- **Family shape:** one map from face names to files. `regular`, `italic`, `bold`, and `bold-italic` keep their meaning, so existing themes and `font-files` stay valid; other weights are `500` or `500-italic`. `400` and `700` are rejected as names, so every face has one key and a later layer replaces it when merged. A face of a collection is `{ "file": ..., "index": n }`.
- **Fallback:** inline emphasis and strong text take the nearest weight by the CSS rule, then the requested style. This keeps every earlier fallback except one: a family with bold-italic but no bold now sets strong upright text in bold-italic instead of regular. Strong text keeps a style weight above 700.
- `--print-config` shows weights as numbers and each font file with its `index`.

### Tracking and capitals

- **Tracking** is a unitless number of em from -0.2 to 1 rather than a length, so tightening is possible and the basis is always the style's size. It follows every character, spaces included, also the last one of a line. Tracked text sets no ligatures.
- **Uppercase** uses Unicode case mapping before shaping, so "ß" becomes "SS" and hyphenation sees the capitals. Inline code keeps its case and spacing.
- Header and footer bands ignored both until [milestone 13](#slots) let band slots name custom styles.

### Custom styles

- **Place:** a top-level `custom-styles` map next to `styles`, not extra entries in `styles`, which stays a fixed list so a misspelled built-in name remains an error.
- **Bases:** `based-on` names a built-in or another custom style. The chain is followed to a built-in style, and a cycle is reported once, at its first member by name. "One level of lookup" in the milestone brief is read as one flat namespace of names, without selectors or context.
- **Kinds:** the built-in style at the end of the chain makes a style a heading, list, or paragraph style, resolved as an enum. `bullets` is accepted only on list styles and `number-gap` only on heading styles, checked when the theme resolves. Names use the characters of `{.name}` and cannot shadow a built-in style.

### Attribute syntax

- **Headings** take one class through pulldown-cmark's heading attributes, alone or with the label. Other attributes and a second class are errors at the attribute. This replaces the milestone 08 rule in [labels and cross-references](#labels-and-cross-references) that heading classes are errors.
- **Paragraphs:** a `{.name}` at the end of the last text is read from the source, so `\{.name}` stays text. When only spaces or quote markers precede it on its line, it stands on a line of its own and styles the list that must start next; otherwise it styles the paragraph. A line of its own after paragraph text without a blank line therefore styles the list, which is what an author writing a label line before a list means.
- **Checks:** the parser has no configuration, so [a pass before layout](../src/layout/classes.rs) reports unknown names and styles of another kind at the attribute. `kyber check` does not parse the body, so these errors show up in `render`.

### Keeping and hanging numbers

- `keep-with-next` marks the last line of the block as a line a page never ends after, as heading lines are. For a list that is its last line.
- `number-gap` hangs a typed "N." followed by a space: the number is set in the heading's face, and the text is set in a frame narrowed by the number's width and the gap, so wrapped lines align with the text. Such headings are not numbered automatically and do not advance the counters, which would otherwise print "1.2.1 1.".

### Sample

[The typography sample](../samples/typography.md) uses [its theme](../samples/themes/typography-styles.json), named apart from the sample so `render-samples.sh` renders it once. Bundled Libertinus Serif has weights 400 and 700 only, so the theme maps 300 and 500 to those files to show the mapping; real Light and Medium faces were checked with installed Avenir Next and Helvetica Neue. Libertinus Serif has no ✓ (U+2713), so the check list uses ✔ (U+2714).

## 2026-10-07: Milestone 12 page geometry, covers, and bands

### Margins and prose width

`page.margins.mirror` (default `true`) is the only switch. [`PageGeometry::left_margin`](../src/config/resolved.rs) picks the left margin per physical page for the body, the bands, and the title page, which is page 1 and so always has the inner margin on the left.

`page.text-width` is a length or `null` for the frame width. `null` was chosen over a sentinel such as `"100%"`, since lengths have no percentages, and it fits the rule that `null` removes something: here the narrowing. `page.wide` lists `table`, `figure`, and `code-block`; it lives under `page` because it only matters together with the text width. A text width that is zero or wider than the frame is an error, and the column gap is now checked against the prose width.

Lines are still broken once, before pagination, at block-level insets. [The flow](../src/layout/mod.rs) starts in a prose frame whose right inset is the difference between frame and prose width, and the frame carries that difference as `widen`. A table, figure, or code block listed in `wide` drops it and spans the frame. Lists, quotations, column sections, and footnotes set `widen` to zero, so a wide block inside them keeps the width it is in. A `keep` group and a `full-width` block of a column section pass the prose frame on, so wide blocks widen there. Column sections divide the prose width. This keeps one rule, "a wide block widens where it stands directly in the prose", without asking the composer to know block kinds.

Prose starts at the inner edge. With mirrored margins that is the right side of even pages, which is only known during composition, so every flow line records whether it is wide, and [the composer](../src/layout/pages.rs) moves prose lines, column regions, footnotes, and the footnote rule right by the difference on those pages while wide lines and their repeated table headers stay put. With the default `null` the shift is zero, which is why existing output is unchanged.

### Slot groups

The title page and each header and footer are arrays of groups `{anchor, offset: {x, y}, width, align, slots}`. Offsets are non-negative and point into the frame: `x` from the anchored side, `y` down from top and middle anchors and up from bottom ones. Signed offsets were not needed, since the anchor already picks the direction. `width` defaults to the frame width less `x`; a group must fit in the frame. `align` overrides the slot styles' alignment and defaults to it, so the default title page keeps its centered title and justified abstract in one group.

Title page groups are placed by their box, including the space above the first slot, as title slots were stacked from the top of the text area. Band groups are placed by baselines on the band baseline that `header-offset` and `footer-offset` already define: top anchors put the first baseline there, bottom anchors the last, middle anchors center them. Boxes would have moved existing band baselines by font metrics, and baselines are what bands align across a page. `header-offset` and `footer-offset` therefore stay.

The old `left`, `center`, and `right` band shape and `title-page.slots` were replaced, not kept alongside: schema version 1 is unreleased, and one shape avoids two code paths. The default theme, the report sample theme, the docs, and the tests were migrated; all nine existing sample renders are byte-identical. This supersedes the band slot `null` of milestone 01 and the fixed slot positions of milestone 08: a band is `null` or an array of groups. A band slot is `{text, style, required, space-before}`; `style` is one optional field from the existing style list, defaulting to the band's `header` or `footer` style, so milestone 13 can widen it to custom names. Image slots stay title-only, since bands align baselines.

Band text never wraps; each line of slot text is one line and too wide a line is an error naming the group and page. Overlap is checked between the line boxes of all groups of a band, not between group boxes, so the default theme's three full-width groups do not collide.

### Lines and metadata

[`Template::fill`](../src/config/template.rs) returns lines. A line break in theme text and each entry of a list value start a line; blank lines and blank list entries are dropped. Line breaks inside a scalar value keep their earlier meaning: paragraphs and spaces on the title page, spaces in bands. Splitting values at single line breaks would have changed abstracts written with hard-wrapped YAML.

Front matter gains `meta`, a map of text or lists of text, merged per key so `--set meta.key=value` replaces one entry. Keys use the label alphabet (letters, digits, `-`, `_`) in both the map and `{meta.key}`, so a key a slot can never name is rejected. Unknown top-level keys stay errors. Numbers must be quoted, as for `date`. `{meta.key}` is accepted in any slot, since only the document knows its keys; a missing value omits an optional slot, and required slots report `{meta.key}` like other placeholders.

`{pages}` is the physical page count. It is rejected in title slots like `{page}`. Bands are drawn after the final layout pass and never change the layout, so the total needs no extra settling pass.

### Consequences

- A wide table narrower than the frame is centered in the frame, which looks off beside narrow left-set prose. Superseded by [milestone 13](#wide-tables-narrower-than-the-frame).
- Figures and tables inside `columns` still take the column width even when listed in `wide`; an author who wants them wide uses `full-width`.
- The sample [offer](../samples/offer.md) with [its theme](../samples/themes/business.json) selects the theme in front matter, so `scripts/render-samples.sh` renders it once.

## 2026-10-07: Milestone 13 rich tables

### List table syntax

`::: table` holds one list: an item per row, each with a nested list holding an item per cell; the first row is the header. Attributes go at the start of an item, `{.name}` on rows and `{.name span=n}` on cells, read from the source like other attributes so `\{` stays text. They may stand alone on the item's line. A row item may hold nothing but its cell list, and cells hold only paragraphs and lists (nested lists included), because headings would enter the outline, and images, code, quotations, and tables in cells would need their own measuring for little use in business documents. Short rows are errors rather than padded, as decided in the amendment.

The caption follows the closing `:::` as a `: Caption {#tbl:x}` paragraph, so it attaches exactly as for pipe tables. It also attaches when written inside the directive after the list, which needed no extra code.

Directive attributes are `{key="value" ...}` after the name, with spaces separating `align` and `widths` entries. Only `table` takes them; any other directive with attributes is an error. A pipe table inside `::: table` takes the attributes too, since the directive only checks that it holds one table; this gives pipe tables `*` widths at no cost.

The document model has one table: `Block::Table` with `columns` (alignment and `auto` or `*` width), rows with an optional class, and cells holding blocks, a span, and an optional class. A pipe table cell is one paragraph.

### Widths, spans, and styles

`auto` columns are fitted as in milestone 07 within what the minimum widths of the `*` columns leave; `*` columns then share the rest equally, each at least its minimum. Without `widths` all columns are `auto`, which is the milestone 07 rule exactly. A spanning cell is measured separately and widens the columns it spans equally where it needs more minimum or natural width than they have; this keeps a group row's long label from wrapping in a table of short columns.

A cell's style is its own class, else the row's, else `table-header` or `table-cell`. Column alignment overrides the row style but not a cell's own class, so a totals label can be right-aligned in a left-aligned column while the amount keeps its column's alignment. A spanning cell takes its first column's alignment. Row and cell classes must be paragraph styles, checked with the other classes before layout.

Cells are laid out by the ordinary block flow in a frame of the cell's width, so lists, list styles, spacing between blocks, and footnote references behave as in the body. A cell of a single paragraph, which every pipe table cell is, is shaped once for measuring and setting as before; going through the flow shaped it twice and made the table benchmark 40% slower.

### Rules

`tables.rule-thickness` and `tables.rule-color` are replaced by `top-rule`, `header-rule`, and `row-rule`, each `{thickness, color}` or `null`. The existing output drew a rule above the header too, so it needed its own setting to stay byte-identical; the default theme sets all three to the old hairline. Schema version 1 is unreleased, so the old keys were removed, as with the band shape in milestone 12. A paragraph style's `rule-below` is `none`, `header`, or `row`, so a row can drop its rule or take the heavier one; a single-value `none` was rejected as an odd enum for no saving. It is accepted only on paragraph styles and read only where the style styles a row.

`keep-with-next` on a row style keeps the row with the next one, which group rows need so they never end a page alone. This follows the style property's meaning for blocks; it was not in the brief.

### Wide tables narrower than the frame

A table in `page.wide` that fits within the prose width is centered in the prose width; a wider one starts where the prose starts and takes the width it needs. Its caption is set across the same width. Centering across the frame put narrow tables off-axis beside left-set prose. Non-wide tables keep centering in their frame, so existing samples are unchanged.

On mirrored even pages a wide line now moves right with the prose as far as the frame allows: `FlowLine::wide` holds the frame width the line leaves free, and the composer shifts it by the smaller of that and the prose shift. A table as wide as the frame does not move, as before.

### Slots

Title and band slot `style` is a built-in slot style name or a custom style name (`SlotStyle`), resolved and checked with the theme. Bands now apply the style's tracking and capitals. Tracked text still adds space after its last letter, so right-aligned tracked band text sits slightly left, as recorded in milestone 11.

### Reference comparison

An offer theme with installed Avenir Next (Gotham, the reference font, is not installed) set the reference's cost table pages from Markdown outside the repository. Page breaks fell after the same rows as in the reference, on three pages, with the same header, group rows, muted detail lists, note row, and totals. Differences: Avenir Next runs wider than Gotham; detail lines hyphenated where the reference does not (a theme choice, `hyphenate: false`); the list markers of Avenir Next are larger than the reference's small bullets; group rows in the reference have more space above. No layout feature was missing.

## 2026-10-07: Follow-up fixes

Fixes for follow-ups from milestones 03 to 13 that needed no product decision.

### Text and line breaking

Not started.

### Pagination

Not started.

### Configuration and CLI

#### Text settings accept numbers

`title`, `subtitle`, `date`, and `abstract` in front matter already read any scalar as its source text, because serde-saphyr hands the raw text to a `String`. `author` and `meta` values deserialize through a visitor, where serde-saphyr offers only the parsed number, so `1.50` becomes `1.5`; those visitors accept integers, floats, and booleans as text. `--set` parses values into `serde_json::Value`, which also loses the text, so for the four plain text keys a number or boolean keeps its trimmed source text. Consequence: quote a decimal in `meta` or `author` to keep trailing zeros.

#### Check runs everything before layout

`kyber check` calls the same `prepare` step as `render`: body parsing, bibliography, citations, document images, and theme images. Layout and font loading stay out, so `check` stays quick and needs no fonts. Consequence: fixtures used with `check` need a valid `.bib` file.

#### Theme images follow the title layout

Only images in the slots of the title layout in use are decoded: the title page groups with `document.title-page`, else the title block when it shows. Header and footer slots hold no images. The file existence check still covers every theme image.

#### Unused font settings warn

Resolution records the font tokens that styles use, following token-to-token references, and warns for a `fonts.*` setting from front matter or `--set` whose token nothing uses. Warnings are diagnostics with a `warning` flag, kept in `Config` and printed after loading. The latest layer that sets a token gets the warning.

#### Table captions at table width

Every table's caption is set at the table's width from its left edge, pipe and list tables alike, wide or not. Before, a caption spanned the frame or, for wide tables, the placement width, so a narrow centered table had a caption wider than itself. A long caption now wraps more over a narrow table. The caption's anchor for references moved to the table's left edge too. This replaces the milestone 13 rule for wide tables.

#### List spacing

The default theme sets the `list` style's `space-before` and `space-after` to `$spacing.block`, the token quotations and code blocks use, so a list after a paragraph and lists of different kinds in a row have a gap. Paragraphs have no spacing token of their own in the default theme (they use an indent). The layout applies list spacing only to a list outside other lists, so a nested list sits at item spacing under its parent item instead of a block gap. Lists in footnotes and in table cells start a new nesting, so they get the spacing against their neighbours in the note or cell; the cell edges drop it (checked in the offer sample), footnotes were not inspected. The sample PDFs `de`, `en`, `offer`, `pagination`, and `typography` changed and keep their page counts.

#### Raster density

The stored density sets a raster image's natural size, each axis on its own, so non-square pixels keep their shape. PNG uses `pHYs` when its unit is the meter. JPEG uses JFIF when its units are inches or centimeters, else the EXIF resolution tags (inches or centimeters, inches by default), found by a small marker scan in `src/image/density.rs`. Files without a usable density keep one point per pixel, so existing images do not change size. This replaces the earlier decision that density is ignored (milestone 06).

#### Paths in messages

Document, theme, bibliography, and resource paths are normalized lexically, with no canonicalization, so symlinks stay as written. Messages print paths inside the working directory relative to it, and others in full. The base is process-wide state set once by the CLI (`show_relative_to`), so `Source` and `Resource` display code needs no extra argument.

### References and structure

Follow-up fixes to milestones 04, 08, 09, and 11.

- **Citation links per work.** The formatter returns each citation as pieces of text, and a piece that shows a work carries the position of that work in the bibliography. Layout links each piece to its entry. In author-date style a work's piece is its name, year, and locator, without separators or brackets. In numeric style a range such as "1–3" links its first and last number, since the middle ones are not shown. A prefix stays outside the link. This replaced one target per citation.
- **Numeric label column.** A numeric bibliography sets its labels in a column as wide as the widest label plus half an em, the gap list markers use, and the entry text after it on every line, like LaTeX's `thebibliography`. Labels are left aligned. `bibliography.hanging-indent` applies to author-date entries only. The label is shaped separately from the text, so the entry text is not preceded by a space.
- **Heading number gap.** `number-gap` is allowed on `heading-1` to `heading-6` as a style field, so the theme can set it per level. Layout sets the automatic number hanging and the text after the gap, the same way it sets a typed number in a custom style. Without it one space follows the number, so the default theme is unchanged. The number text in contents entries, bookmarks, and running headers does not change, because `Heading::title` still joins number and text with a space. A custom style based on a heading style inherits the gap for its automatic numbers, and its own `number-gap` still means that the author types the number.
- **Footnote lists and code.** Inside a note, `list`, `lists.indent`, `lists.item-spacing`, and `code-block` are scaled by footnote size over body size: font size, spacing, and indents. A ratio keeps the theme's proportions; sizes in em of the footnote style would have needed new theme fields. Custom list styles and quotations inside notes stay at their own size.
- **Citation prefixes.** In a bracket, text before the first `@key` of an item is its prefix, as in Pandoc: `[see @a, p. 3; also @b]`. A key counts at a word start, so `[mail me@x.org]` is text, and a bracket without any `@key` is text. Prefixes are per item. In author-date style the output is "(see Smith 2024, p. 3; also Lee 2022)". In numeric style the prefix goes inside the brackets, before the number: "[see 1, p. 3; also 2]". Numbers are sorted and grouped into ranges only when no item has a locator or a prefix, so the order the author wrote is kept. Suffixes after the locator are not supported.
- **Contents level styles.** `toc.level-styles` lists style names, built-in or custom, for the entries of level 1, 2, and so on, and the last repeats for deeper levels. Absent or empty means `toc-entry` for all, so the default theme is unchanged. The indent per level and the leader still come from `toc.level-indent` and `toc.leader`, measured in the `toc-entry` size. An unknown name is an error at its list position.
- **Headings in footnotes.** The parser rejects them at the heading, like tables and images in footnotes. Layout no longer has a path for them.

## Recording a decision

Add a short dated entry when a choice affects future work. State the choice, reason, affected interface or behavior, and any unresolved consequence. Link to code, schema, or tests once they exist. Replace superseded guidance with a reference to the newer decision.

Keep dependency versions and runnable commands in their actual manifests and tooling. Do not copy them here. Record why a dependency was chosen when that reason is not evident from the code.
