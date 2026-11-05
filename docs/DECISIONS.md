# Implementation decisions

The product choices are authoritative in [the phase 1 briefing](BRIEFING.md) and [the phase 2 briefing](RUST_CRATE_API_PHASE_2.md). This file records implementation choices made within those contracts.

## 2026-10-07: Variable font weights and italics

- **File syntax:** keep the face map and add `{ "file": "font.ttf", "variable": true }` under `regular` or `italic`. `index` is optional for variable files and defaults to 0; collection objects without `variable` still require it. This avoids another family format and preserves resource origins. Explicit faces and `null` entries override generated instances.
- **Instances:** expand the `wght` range into the existing 100 to 900 weight steps. An `ital` axis supplies 0 and 1 when supported; otherwise the face entry supplies the style, including separate italic variable files. Installed lookup reads axis ranges and exposes the same instances automatically. Block requests are checked after loading and fail when absent; inline emphasis and strong text retain nearest-face matching.
- **Rendering:** use Rustybuzz 0.20.1 `set_variations` and Krilla 0.8.2 `Font::new_variable` with identical coordinates. Krilla's existing variable-font subsetter embeds static instances. No dependency or feature change is needed. Width, slant, optical size, named instances, arbitrary axes, and weights between the existing steps are deferred; their axes keep font defaults.
- **Fixtures:** Public Sans upright and italic variable files under [test fixtures](../tests/fixtures/fonts/README.md), with their OFL license. They are test inputs, not bundled runtime fonts. The `ital` range test changes the fixture's axis in memory; visual acceptance uses the original upright and italic files.

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

`druck check <doc>` validates and can `--print-config`; `druck render <doc>` fails until milestone 02. Exit code 0 is success, 1 means diagnostics were reported, and 2 is a usage error.

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

The PDF has no creation date, so repeated renders are byte-identical. Title, authors, and language go into the document metadata. Superseded in part by [PDF document info](#2026-10-07-pdf-document-info). `druck render <doc> [-o PATH]` writes next to the document with a `.pdf` extension by default; `-o` is relative to the working directory.

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

Required title slots are checked as soon as the configuration and metadata are known, by `druck check` and by `druck render`. The error names the document and the slot property.

### Page numbers, variants, and running headers

The displayed page number is the physical page number, counted from 1 and including the title page. It is the number a PDF viewer shows, and odd displayed numbers are odd physical pages, which milestone 01 chose for parity. There is no separate front matter numbering, except in books with parts since [book structure](#2026-10-09-book-structure). As a consequence the length of the title page and the table of contents moves the numbers of body pages, which the settling below takes care of.

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
- **Release builds:** [release.yml](../.github/workflows/release.yml) builds x86_64 and aarch64 Linux on Ubuntu 22.04 images (older glibc), aarch64 and x86_64 macOS, and x86_64 Windows on manual dispatch or `v*` tags, renders the samples with each runnable binary, and uploads archives with SHA-256 files. It did not create a GitHub release; see [name, license, and releases](#2026-10-07-name-license-and-releases).
- **Not set:** `rust-version`, because no older toolchain was tested. `license` was set later: see [name, license, and releases](#2026-10-07-name-license-and-releases).

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
- **Checks:** the parser has no configuration, so [a pass before layout](../src/layout/classes.rs) reports unknown names and styles of another kind at the attribute. `druck check` does not parse the body, so these errors show up in `render`.

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

The owner decided the remaining ones the same day. Built: a `marker` style for list markers, captions at the table's width, Pandoc-style citation prefixes, `document.duplex`, `toc.level-styles`, an error for headings in footnotes, stored raster density, block spacing around lists in the default theme, line-end hyphens that survive text extraction (reverted again, see [owner follow-ups](#owner-follow-ups)), and `page.column-change-spacing`. Kept as they are, by choice:

- List table cells hold paragraphs and lists only.
- A `.bib` `type` field prints as written; an absent one uses the localized label.
- `{section}` shows the first section that starts on the page, as LaTeX's right mark.
- A keep group too tall for a page stays an error; its images are not shrunk.
- The page-scoring trade-offs recorded for the images sample, short pages before tall images, uneven final columns at a keep group, and heading columns ending a line apart stay; tuning one tends to regress another.
- German compounds hyphenate at any pattern point; better data would need a new dependency.
- No settling cache, single PNG decode, or theme settings for breaking constants until a document needs them.
- `margins.mirror` stays theme-only, and captions do not hold footnotes.

### Text and line breaking

- **Tracking at line ends.** Tracking still follows every character, but alignment measures a line up to its last glyph. A shaped run records its letter spacing, and the space after the last character of a line may pass the measure, like a margin hang. Before a hard break or the paragraph end only that space may pass it, not punctuation, as before. Right-aligned and centered tracked text, in paragraphs and in band slots, now ends exactly at its edge; the offer's "AMOUNT" header and its "OFFER" footer line up with the text below them.
- **Final breaking pass.** Badness is no longer capped at 10 000. The first pass is unchanged, since it only accepts badness up to 200. In the final pass, very loose lines now compare: a nearly empty first line costs more than a fuller one, so "Knuth," alone or "Version / 16.0" no longer win a tie. Only a line that falls short with no stretch at all, such as a single word in a justified line, still counts 10 000, because its ratio is infinite. TeX's emergency stretch was tried first, at a quarter of the line width: it also ends the ties, but it shrinks every badness in the final pass, so hyphens became dearer than loose spacing and narrow justified columns in `samples/columns.md` came out visibly looser. Uncapped badness changes nothing in those paragraphs, because none of their lines reached the old cap.
- **Cross-references.** Their text (label and number, or "page 7") never breaks inside, which the resolved text marks with `InlineStyle::unbreakable`. A line may still end before it. Citations still hyphenate, since author names may.
- **Protrusion runs.** A run of protruding characters at a line edge hangs by the sum of their shares in the [protrusion table](#optical-margin-alignment), so the closing quote before a comma hangs too. Three periods at a line end now hang by more than a single period; "…" is one character and does not protrude.
- **URL breaks.** Link text that spells out its URL, in prose, autolinks, and bibliography entries, may break after a `/` that has a character other than `/` on both sides, without adding a hyphen. So "https://" stays whole and a trailing slash stays with its URL. Breaks before `.` or after `-` were left out: slashes were enough for every URL in the samples, and DOIs always have one. A URL break costs the same as a hyphen but is not counted as one for consecutive hyphens. A URL with no such slash that is wider than its column is still an error at its entry. Bibliography entries now fill their lines and break URLs where they reach the edge, as biblatex does, instead of starting a URL on a new line.
- **Hyphens in extracted text.** Readers such as `pdftotext` take a `-`, a soft hyphen, or U+2010 at a line end for an added hyphen and drop it when they join the lines; all three were tried with poppler 26.10. Added hyphens therefore keep `-`, so "typographi-" and "cal" extract as "typographical", in pdftotext and in viewers without `ActualText` support. A hyphen of the text that ends a line, as in "state-of-the-" or "pre-", was first mapped to U+2011 so that readers kept it, but krilla 0.8.2 takes the first text it sees per glyph for the font `ToUnicode` map, so a document whose first hyphen glyph ended a line extracted every `-` of that face as U+2011. The owner reverted that part: a hyphen of the text at a line end is written as `-` again, and "state-of-the-" then "art" extracts as "state-of-theart" in readers that join lines. The full fix waits for `ActualText` across the line break, which belongs with the deferred accessibility work, since the two halves of a line are separate text runs.
- **List marker style.** `lists.marker` and a list style's `marker` name a built-in or custom style whose font, size, weight, and color set bullets and numbers, as the owner decided. Tracking, capitals, and the style's spacing do not apply to markers. A marker style is kept by name and looked up at layout, like a slot's custom style, so it may name a custom style defined after the list style. Markers keep the item's first baseline and end half an em of the list style before the text, so the hanging indent does not depend on the marker style. The default theme sets `lists.marker` to `null` and is unchanged.

### Pagination

**Stranded lines are a rule for pages.** A page never ends after a block's first line or before its last line. The 5 000 costs of [milestone 04](#page-breaks) lost to the square fill cost whenever avoiding them moved a tall unit, such as a heading with its first line or a table row, or added a line to a page that was short anyway, where one more line costs about 2 000 times the shortfall. Page 4 of `samples/offer.md` ended with "Optional work" and one line, and page 4 of both report renders started with a paragraph's last line. Such lines are now `Break::Avoid` in [the flow](../src/layout/mod.rs): no page ends there, but a column split or a continued footnote may, at the old cost, so column balancing keeps its milestone 05 trade-off. A heading, or a block kept with the next one, keeps the first two lines of the paragraph after it, in columns too.

Consequences: paragraphs of two or three lines no longer split across pages. The offer now runs to six pages, the last holding the closing two lines, because the fifth is full; trimming the sample text would bring it back to five. The 52-copy `samples/en.md` benchmark went from 103 to 105 pages.

**Table rows** follow the same rule, applied to the first and the next-to-last body row. Where two rows with the repeated header, and the caption for the first row, are taller than the text area, the row keeps the soft cost, so tall rows never become an error. Kept rows stay kept.

**Hyphenated lines** carry a `hyphenated` flag from the breaker, set where a line ends inside a word. A page or column ending after one costs 1 000 more, as much as a page one line short, after TeX's `\brokenpenalty`.

**Introducing colons.** A page or column ending after a paragraph whose text ends in ':' directly before a list or code block costs 500 more. It only tips close cases; `keep` still forces it.

**Notes** split at the break cost of the note line they stop after, plus the 2 000 for continuing, so they follow the body text rules at the same costs. For each page end the composer also tries stopping a continued note one line earlier, so a single last line can move with the rest. Notes keep soft costs because the check before the search only guarantees that a note's first line fits.

**The page before the report's column section.** In `samples/report.md` and its theme render, the widow rule fixed it: page 3 now holds the whole paragraph and starts the column section. `samples/report-de.md` is unchanged: page 3 ends about a quarter short and page 4 starts with the paragraph's last two lines, which the rules allow. Pages 3 to 5 together fall about 31 lines short, because the full-width chart and the tall drawing cannot split, and the square fill cost spreads that over the three pages (about 10, 7, and 15 lines). Ending page 3 two lines later lowers its cost from 97 767 to 62 197 but raises that of page 4 from 48 056 to 125 694. This is the trade-off accepted for the images sample, so it stays.

**Duplex.** `document.duplex`, default `false`, starts the body on an odd page: a blank page follows the title page, and the table of contents ends its page and is followed by a blank page when the next page would be even. [The composer](../src/layout/pages.rs) inserts that blank page while rendering, so parity, margins, anchors, `{page}`, and `{pages}` all count it. Blank pages have no bands, and the `first` variant applies to the first page after the title page and its blank page. Without a page break after the contents in duplex, the body would start on the same page and the setting could not hold, so duplex ends the contents page itself.

**Column change spacing.** `page.column-change-spacing` is the least space between a column section and the full-width blocks around it, which reused `page.column-gap` before. Its default `null` means the column gap, so themes that set only the gap render as before.

### Configuration and CLI

#### Text settings accept numbers

`title`, `subtitle`, `date`, and `abstract` in front matter already read any scalar as its source text, because serde-saphyr hands the raw text to a `String`. `author` and `meta` values deserialize through a visitor, where serde-saphyr offers only the parsed number, so `1.50` becomes `1.5`; those visitors accept integers, floats, and booleans as text. `--set` parses values into `serde_json::Value`, which also loses the text, so for the four plain text keys a number or boolean keeps its trimmed source text. Consequence: quote a decimal in `meta` or `author` to keep trailing zeros.

#### Check runs everything before layout

`druck check` first called the same `prepare` step as `render`: body parsing, bibliography, citations, document images, and theme images, with no layout and no fonts. The owner replaced this: see [check lays out](#check-lays-out).

#### Theme images follow the title layout

Only images in the slots of the title layout in use are decoded: the title page groups with `document.title-page`, else the title block when it shows. Header and footer slots hold no images. The file existence check still covers every theme image.

#### Unused font settings warn

Resolution records the font tokens that styles use, following token-to-token references, and warns for a `fonts.*` setting from front matter or `--set` whose token nothing uses. Warnings are diagnostics with a `warning` flag, kept in `Config` and printed after loading. The latest layer that sets a token gets the warning.

#### Table captions at table width

Every table's caption is set at the table's width from its left edge, pipe and list tables alike, wide or not (but see [captions over narrow tables](#owner-follow-ups)). Before, a caption spanned the frame or, for wide tables, the placement width, so a narrow centered table had a caption wider than itself. A long caption now wraps more over a narrow table. The caption's anchor for references moved to the table's left edge too. This replaces the milestone 13 rule for wide tables.

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
- **Footnote lists and code.** Inside a note, `list`, `lists.indent`, `lists.item-spacing`, `code-block`, and, since the owner follow-ups, `quote` and custom list styles are scaled by footnote size over body size: font size, spacing, and indents. A ratio keeps the theme's proportions; sizes in em of the footnote style would have needed new theme fields.
- **Citation prefixes.** In a bracket, text before the first `@key` of an item is its prefix, as in Pandoc: `[see @a, p. 3; also @b]`. A key counts at a word start, so `[mail me@x.org]` is text, and a bracket without any `@key` is text. Prefixes are per item. In author-date style the output is "(see Smith 2024, p. 3; also Lee 2022)". In numeric style the prefix goes inside the brackets, before the number: "[see 1, p. 3; also 2]". Numbers are sorted and grouped into ranges only when no item has a locator or a prefix, so the order the author wrote is kept. Suffixes are described under [owner follow-ups](#owner-follow-ups).
- **Contents level styles.** `toc.level-styles` lists style names, built-in or custom, for the entries of level 1, 2, and so on, and the last repeats for deeper levels. Absent or empty means `toc-entry` for all, so the default theme is unchanged. The indent per level and the leader still come from `toc.level-indent` and `toc.leader`, measured in the `toc-entry` size. An unknown name is an error at its list position.
- **Headings in footnotes.** The parser rejects them at the heading, like tables and images in footnotes. Layout no longer has a path for them.

### Owner follow-ups

Decided by the owner after the fixes above.

#### Check lays out

`druck check` is `render` without writing the PDF: it loads fonts, lays the document out, and reports every error `render` reports, such as a word wider than its line, an oversized row or keep group, or a layout that does not settle. Both commands share one `typeset` function in `src/main.rs`, so nothing is duplicated. The title slot check is part of layout, so `check` no longer calls it apart. Consequences: `check` needs the fonts the theme names and takes as long as `render`. `--print-config` stays a view of the resolved configuration and does not lay out, so it also works for themes whose fonts cannot be loaded; the success message "document is valid" is still true.

#### Hyphens in extracted text

The explicit line-end hyphen mapping described under [text and line breaking](#text-and-line-breaking) is removed. Automatic hyphens are unchanged.

#### Footnote scaling

`NoteStyles` also holds the scaled `quote` style and a scaled copy of every custom style based on `list`, by name. Layout picks them inside notes like the scaled `list` and `code-block`. A list marker style, named by `marker`, is still not scaled in notes, neither for the built-in nor for custom list styles, and custom paragraph styles in a note keep their own size. Both are follow-ups if a document needs them.

#### Citation suffixes

Text after the locator in a bracketed item is its suffix, in Pandoc's manner: `[@a, p. 3, emphasis added]` reads "(Smith 2024, p. 3, emphasis added)". The text after the comma is a locator, then a suffix, when the part up to the next comma is a locator (`p.`, `pp.`, or `S.` with a page or range). Otherwise the whole text is the suffix: `[@a, emphasis added]`. A suffix is kept as written, joined to the locator with ", ", and is part of the work's link. Pandoc's manual, "Citation syntax", shows `[@doe99, pp. 33-35, 38-39 and passim]`, with all text after the key handed to its citation processor. Druck supports three locator labels and splits at the first comma instead: that example is the locator "pp. 33–35" and the suffix "38-39 and passim", which prints the same. Three deliberate differences: the text must follow a comma, where Pandoc also accepts `[@a and passim]`; a malformed locator such as `p.3` is silently a suffix, where Druck used to report an unsupported locator; and `[@a see below]` stays an error, now saying that a locator or suffix follows a comma. A narrative citation takes the same in its brackets, `@a [p. 3, passim]`, but brackets that do not start with a locator stay text. Numeric style prints the suffix inside the brackets after the locator, "[1, p. 3, emphasis added]". Numbers are no longer grouped into ranges when an item has a suffix.

#### Captions over narrow tables

A caption is as wide as its table, unless the table is narrower than a third of its frame's width, where the caption runs from the table's left edge to the frame's right edge. The owner's wording was "set at the frame width, left edge still the table's, clamped to the frame". Both cannot hold, since a frame-wide caption can only start at the frame's left edge, so the table's left edge is kept and the width is what remains of the frame. The frame is the one the table is set in: the text area for wide tables, the column in columns. The caption's minimum width is still its longest word.

#### Business theme

`samples/themes/business.json` has a `detail-marker` custom style, 0.6em of the body size in the muted color, which the `detail` list style names as its `marker`. The cost table's detail bullets are smaller than their text. The offer stays at 5 pages.

## 2026-10-07: Placeholders, statistics, and drafts

Built at the owner's request: text statistics, the build date, section page counts, and drafts with a watermark. Authors' rules are in [authoring](AUTHORING.md#placeholders), themes' in [templates](THEMES.md#placeholders) and [watermark](THEMES.md#watermark).

### Placeholders in the body

A `{name}` in Markdown text is a token, `Inline::Field`, which layout replaces with its value in the style of the text around it. Statistics are counted from the parsed document before layout, so values are final before the first pass and never change the page count they might depend on. Two rules keep values free of circular references:

- Placeholders count as no text. A count never includes its own digits or another count's, so `{words}` is the same whether the text shows it once or ten times.
- Placeholders that depend on the page (`{page}`, `{pages}`, `{section}`, `{subsection}`, `{section-page}`, `{section-pages}`) are rejected in the body. `{pages}` in the text would change the layout that decides it; the settling loop could carry it, but no document has needed it.

Only a brace around a name (a letter, then letters, digits, `-`, `_`, `.`) is a token, so `{.name}`, `{span=2}`, and `{a, b}` stay as before. An unknown name is an error rather than text, since a typo such as `{wrods}` would otherwise print silently. `\{` escapes. Headings reject placeholders, because their text feeds contents, bookmarks, and running headers before layout. A placeholder without a value, including `{draft}` outside a draft, is an error at its location, so draft-only text cannot reach a final render.

### Counting

[`statistics`](../src/statistics.rs) uses `unicode-segmentation` for UAX #29 grapheme clusters, words, and sentences: hand-written splitting on spaces miscounts dashes, combining accents, and emoji, and the crate is small, has no dependencies, and is already the standard. On top of UAX #29:

- Words joined by one hyphen count once, as word processors and dictionaries treat "well-known". UAX #29 splits them.
- A sentence does not end after a single letter, a short per-language abbreviation list, or in German a number before a month or "Jahrhundert". UAX #29 breaks after "Dr." before a capital. A general German ordinal rule was tried and dropped: "S. 12. Das" is a sentence end, and German nouns are capitalized, so only the noun after the number can tell them apart.
- Generated text (heading numbers, citations, cross-references, footnote markers) is not counted, as LaTeX's `texcount` does by default. Citations would otherwise change counts with the citation style. Headings, table cells, and captions count toward characters and words but are not sentences or paragraphs.
- Reading speed is 228 words a minute in English and 179 in German, both from one study (Trauzettel-Klosinski and Dietz 2012, IReST), so the two languages compare. It is not configurable yet.

### Build date

`{build-date}` and `{year}` use the UTC day of the run, without a date dependency. `SOURCE_DATE_EPOCH` fixes it, which keeps reproducible builds byte-identical. The local time zone is not used: the standard library cannot read it, and around midnight the date can differ from the local one by a day. Documents without these placeholders are unaffected, so sample renders stay byte-identical.

### Drafts and the watermark

`document.draft` (front matter `draft`) gives `{draft}` the new `draft` label. The new theme section `watermark` is `null` or `{text, angle}`, set in the new `watermark` block style; it is not set while a placeholder in its text lacks a value. The default theme uses `{draft}`, so a draft gets "DRAFT" and a final document nothing, with no separate switch. This follows LaTeX's `draftwatermark`: centered, 45 degrees, light gray, behind the content. The text shrinks to fit within 90% of the page, so "ENTWURF" fits on A5 without a theme change. Blank pages get none, like bands. A new page item, `TurnedText`, carries the angle to the PDF writer, which turns the run about its baseline start.

## 2026-10-07: Name, license, and releases

At the owner's request, ahead of publishing on GitHub as [coderscantina/druck](https://github.com/coderscantina/druck).

- **Name:** the project, crate, and binary are `druck`, German for "print". The fully qualified name is `com.coderscantina.druck`, for any place that needs a reverse-domain identifier, such as a future macOS bundle or package manager entry. The sample texts, fixtures, and the sample logo say Druck too. The bibliography fixture's institution is "Kurrent Lab" so the sort order the tests expect is unchanged.
- **License:** MIT, © 2026 Michael Wallner, Coders Cantina, in [LICENSE](../LICENSE) and `Cargo.toml`. This lifts the distribution block of milestone 10.
- **Versions:** calendar versions `YY.M.D` in `Cargo.toml`, since cargo needs semver there. The tag is `vYY.M.D-HASH` with the release commit's seven-character hash, so two releases on one day get distinct tags. [build.rs](../build.rs) adds the same hash to `--version`, and [scripts/package.py](../scripts/package.py) to archive names, so a binary, its archive, and its tag name the same build. Builds outside a git checkout print the plain version. The changelog heading has no hash, because the commit that writes the changelog cannot know its own hash.
- **Releases:** [scripts/release.sh](../scripts/release.sh) follows the owner's release script for ravn: it releases from an up-to-date `main`, bumps the version, commits, tags, and pushes. It also writes `CHANGELOG.md` from commit subjects grouped by gitmoji, in plain shell, so releasing needs no extra tool. The tag push lets [release.yml](../.github/workflows/release.yml) create the GitHub release with the archives and the newest changelog section.
- **Schema `$id`:** the raw GitHub URL of `schema/theme.v1.schema.json` on `main`, which resolves once the repository is public.

## 2026-10-07: Named user themes and PDF reference themes

The CLI accepts a bare theme name in front matter or `--theme` and loads `<name>.json` from the user's config directory. Unix, including macOS, uses absolute `XDG_CONFIG_HOME` or `~/.config`, matching the owner's requested macOS location. Windows uses `APPDATA`. The theme directory below either root is `druck/themes`. No dependency is needed for these environment-based paths. Explicit paths retain their original bases; an existing local file with the exact bare name wins. Only ASCII letters, digits, hyphens, and underscores count as a name, so path traversal and filenames with extensions remain explicit paths. Assets keep the selected JSON file's origin. See [lookup](../src/theme_path.rs), [CLI tests](../tests/cli.rs), and [the guide](THEMES.md#named-themes).

Personal themes were installed in `~/.config/druck/themes`, with metadata instructions in its `README.md`. `coderscantina` follows the supplied offer with Bad faces instantiated from the installed variable fonts, because installed lookup currently sees only their default faces. The original fonts are unchanged. `latex-1`, `latex-2`, and `latex-3` remain separate: their body sizes are respectively 12, 11, and 10 TeX points, their side margins differ, and the third has a full-width institutional title above two-column prose. They use bundled Libertinus at the owner's request. Columns belong to the document directive. These themes and the personal Bad assets are not binary resources or tracked repository files.

The current layout cannot reproduce two reference details through themes alone: cover and body margins share one frame, and leading space on the first title-block slot is dropped at a page top. The theme notes record those limits. Fixing them is a follow-up, outside named lookup.

## 2026-10-07: PDF document info

The document info and XMP metadata now take more from the document, still without reading the clock, so the same input and version give the same bytes. See [`document_info`](../src/pdf.rs).

- **Title:** front matter `title`, else the text of the first level 1 heading among the top-level blocks, without number, styles, or notes. Untitled PDFs otherwise show their file name in viewers. Headings in quotes or columns are not searched, and a document whose first H1 is a section such as "Introduction" gets that as its title; a front matter `title` overrides it.
- **Subject:** `abstract`, else `subtitle`, with whitespace and blank lines collapsed to single spaces.
- **Creation date:** only when `date` is exactly `YYYY-MM-DD` and the day exists. Free text such as "7 October 2026" writes no date and is not an error. krilla writes the same value as the modification date, as midnight UTC.
- **Creator and producer:** `Druck <version>` and `Coder's Cantina Druck <version>`, from the crate version, following the vendor-first pattern of Adobe and Microsoft. A reverse-DNS identifier was rejected because viewers show these fields as text. PDFs therefore differ between releases.
- **Keywords** are not written. Markdown has nothing to infer them from; an explicit front matter key would be a separate decision.

## 2026-10-09: Book structure

At the owner's request, outside the milestone plan, after comparing Druck with three LaTeX book templates (a memoir novel, a memoir ebook, and a book-class novel with drop capitals). The syntax stays close to plain Markdown: three directives, Pandoc's heading attributes, and thematic breaks. Everything else is theme settings.

- **Parts:** `::: front-matter`, `::: main-matter`, and `::: back-matter` follow the existing directive names. Each starts a new page, an odd one with `duplex`, and may appear once, in this order, outside other directives. The theme's `matter` section sets each part's page number format (`decimal`, `decimal-leading-zero`, `lower-roman`, `upper-roman`, after CSS list styles) and whether it restarts the count. The default theme numbers front matter i, ii, and main matter 1, 2, with back matter continuing it, as LaTeX's `\frontmatter` does. Headings in front and back matter have no number and do not count. Pages before the first part have no number and use the `title` page variant, so half titles, copyright pages, and dedications have no running heads. A document without parts is numbered as before, byte for byte.
- **Shown numbers:** settling compares the shown page numbers as text, so the table of contents and page references show "iv". `{pages}` is the last number of the page's count, so "1/3" in main matter ignores the front matter. A page reference to a page without a number is an error at the target, since it would show nothing. PDF page labels carry the numbers only in books with parts. Unnumbered pages get an empty label. krilla 0.8.2 writes one label entry per page because of how it compares offsets; that is valid and was not worth working around.
- **Headings:** `{-}`, `{.unnumbered}`, and `{.unlisted}` follow Pandoc. pulldown-cmark 0.13 drops one-character attributes, so `{-}` is read from the heading's source. Unnumbered headings do not count; unlisted ones stay in the outline.
- **Chapters:** a level 1 heading in the body opens a chapter, including the bibliography heading. The theme's `chapters` section sets `break-before` (`none`, `page`, `recto`), a `sink` from the top of the text area, the chapter number format (also inside "II.3"), the number on its own line in `number-style` with an optional `chapter` label, and an ornament image. A break is skipped inside a keep group. The sink is an empty line kept with the heading, since space at a page top is dropped. A number above the heading takes the heading's space before; its style's space after separates it from the heading text. References still read "Section 2"; see follow-ups.
- **Opening page variant:** `pages.opening` comes first in the chain of a page where a level 1 heading starts, before `first`.
- **Scene breaks:** `***`, `---`, and `___` were errors and are now scene breaks, so no document changes meaning. The theme sets the mark as text in the new `scene-break` style or as an image. The mark stays with the next line. With `blank`, the break keeps the mark's room but draws it only at the top of a page, where an empty line would go unnoticed. A paragraph after a break has no indent.
- **Drop capitals and lead-ins:** `chapters.drop-cap` (2 to 5 lines) and `chapters.lead-in` (words) apply to the first paragraph after a chapter heading without a custom style. Styled paragraphs such as epigraphs pass the opening on; any other block ends it; `{.no-drop-cap}` turns it off for one paragraph without naming a theme style. The capital is the first letter with any punctuation before it, in the paragraph's font and weight, upright, sized so that it reaches from the cap height of the first line to the baseline of the last. Its lines are indented and stay on one page; the line breaker takes a count of narrow lines for this. A paragraph shorter than the capital gets none. The lead-in uses the font's `smcp` feature, or capitals at 80% size for fonts without it. Lead-in words are never hyphenated, because a cut word is reshaped without the feature.
- **`::: toc`:** places the table of contents and implies it. Without the directive, `toc: true` keeps its place after the title block.

Not included: real small capitals as a style property, a separate font for the drop capital, multi-page front matter templates from metadata, covers with bleed, and chapter labels in references. These were proposed and left for later.

## 2026-10-09: Print marks and bottom-aligned pages

At the owner's request, to release books, together with EPUB output.

- **Bleed and crop marks:** `render --bleed LENGTH` and `--crop-marks` are command-line options, not theme settings, because they belong to one print job, not to the design. The theme's page size stays the trim size, so layout does not change; only [the PDF writer](../src/pdf.rs) adds them. The media box grows by the bleed, or with marks by the mark offset plus the mark length, and each page gets a TrimBox and a BleedBox. Without either option no box is written and the PDF is byte-identical to before. Marks follow InDesign's defaults: 0.25pt thick, 15pt long, starting 6pt from the trim edge or at the bleed edge if that is farther, in the registration color (the `All` separation), so they print on every plate. Content never reaches the trim edge today, since everything sits in the margin frame, so nothing is extended into the bleed. Full-bleed images or backgrounds would need that.
- **`::: bottom`:** a keep group set at the bottom of the text area, above footnotes, after which a new page starts. The owner chose it over a LaTeX-style `\vfill` spacer, which is more general but harder to follow across page breaks. Pagination needs no change: the group is a keep group followed by a forced break, and the page's lines are only moved down when it is rendered. It is only allowed outside other directives, and `page-break`, `bibliography`, `toc`, and `columns` are not allowed inside it, as inside `keep`. The book sample's copyright page uses it.
- **Duplex title page in books:** a book with parts gets no blank page after a duplex title page. The parts start odd pages themselves, so the body still opens on a right-hand page, and the pages before the first part follow the title page directly: the copyright page lands on its back, page 2, as in printed books. A document without parts keeps the blank page, since its body must start on an odd page. This changes the duplex rule of the [follow-up fixes](#2026-10-07-follow-up-fixes) for books only.

## 2026-10-09: EPUB output

At the owner's request, outside the milestone plan, so authors can release a book as a print PDF and an ebook from one source. The code is in [`src/epub`](../src/epub); authors read [authoring](AUTHORING.md#epub), theme authors [themes](THEMES.md#epub).

- **Format:** reflowable EPUB 3, written without layout. It reuses the parsed document, the formatted citations, [`Structure`](../src/layout/structure.rs) for numbers, anchors, and reference text, and [`Fields`](../src/layout/fields.rs) for placeholders. Layout's checks that do not depend on pages run first, through a shared `layout::check`. The owner chose reflowable over fixed layout, which would throw away what readers do well.
- **Command line:** `render` picks EPUB when the output path ends in `.epub`, in any case. There is no `--format` flag: `-o book.epub` reads well, and a flag would only matter for the default output path, which stays `.pdf`. `check` keeps checking the PDF layout, a superset of the EPUB checks except `meta.isbn`.
- **Files:** a new XHTML file starts at each level 1 heading, the bibliography, each part, and each top-level `::: page-break`, since a new file is the one page break every reader honours. A page break nested in another directive becomes a CSS break. Files are `text-001.xhtml` onward, flat in `EPUB/` with `style.css`, `nav.xhtml`, `fonts/`, and `images/`. An empty document still gets one file, since a spine needs one.
- **Contents:** `nav.xhtml` is the only table of contents. Where `::: toc` stands, or after the title page with `toc: true`, it also joins the spine as the contents page, so there are not two lists to keep in sync. Without listed headings it links the first file. There is no NCX: EPUB 3 readers do not need it and no target reader was found that does. Landmarks name the cover, the contents page, and the first body matter file.
- **Matter:** each file's `body` carries `epub:type` `frontmatter`, `bodymatter`, or `backmatter` from its part. Content before the first part is front matter; a document without parts is all body matter. Body matter files that open with a level 1 heading wrap it in a `chapter` section, and the bibliography in a `bibliography` section.
- **Page references:** a reflowable book has no pages, so a page reference shows the target's reference text, as a plain cross-reference does, and links to it. Showing the PDF's page numbers would tie the EPUB to a PDF layout run; leaving the reference out would break sentences. Sentences such as "starts on [@sec:x, page]" read awkwardly in the EPUB; print page numbers are left for later.
- **Footnotes:** each note is an `aside` with `epub:type="footnote"` at the end of the file that references it, with its number linking back. The parser allows one reference per note, so a note has one home.
- **Stylesheet:** sizes are `rem` of the body size and spacing is `em` of the element's own size, so readers can scale the text. Containers carry the text properties of their frame style and paragraphs inherit them, which mirrors how the PDF sets a paragraph in its frame's style. Custom and slot styles are `s-name` classes. The drop capital uses `initial-letter` inside `@supports`, with a floated letter as the fallback.
- **Fonts:** bundled fonts and font files are embedded whole, in their own format, without subsetting or obfuscation. A variable file is declared for its `wght` range; its `ital` axis is not mapped, because CSS selects italics by face, not by axis. Collections are skipped with a warning, since readers load single faces only. Installed fonts are named with a generic fallback and a warning, never embedded, because their licenses rarely allow it. Installed fonts are therefore not looked up for EPUB.
- **Metadata:** the title falls back to the first level 1 heading, then the file name, since `dc:title` is required. `meta.isbn` becomes `urn:isbn:` and is checked for 10 or 13 digits; it needs no new key and is usually printed on the copyright page anyway. Without it the identifier is a version 8 UUID from a 128-bit FNV-1a hash of the title, language, and authors: stable across platforms and releases without a hash dependency, and unchanged by edits to the text. `dcterms:modified` is midnight of the `SOURCE_DATE_EPOCH` day, else of an ISO `date`, else of today.
- **Cover:** the new front matter key `cover` names an image relative to the document, checked like other resources and decoded like body images. It is the smallest addition; the PDF ignores it.
- **Container:** a small ZIP writer in [`zip.rs`](../src/epub/zip.rs) with fixed timestamps, no extra fields, and entries in a fixed order, so the same input gives the same bytes. `mimetype` comes first, stored. Text and fonts are deflated with `flate2`, which krilla and png already link, so the binary gains no new crate; PNG and JPEG are stored. The `zip` crate was not used: it brings more than the dozen lines of headers Druck needs.

Not included: page references with print page numbers, font subsetting, a cover from the theme, and `meta.publisher` and other metadata beyond the front matter keys. Accessibility metadata was added later; see [tagged PDF and conformance](#2026-10-12-tagged-pdf-and-conformance).

## 2026-10-11: Inline code background and padding

- **Syntax:** `inline.code.background` (a color or `null`) and `inline.code.padding` (a length, em of the surrounding text, like the other inline lengths). The defaults, `null` and `0pt`, keep existing output unchanged.
- **Layout:** padding widens the first glyph of a span's first word and the last glyph of its last word, so breaking, alignment, and drawing measure it without special cases. Code never breaks inside a word, so the padding is never reshaped away. The box is a filled rectangle from the code's ascender to its descender, extended across spaces inside the same span and started again on each line.
- **Deferred:** rounded corners. `Item::Rect` has none, and square boxes print cleanly; add a radius when a theme needs it.
- **EPUB:** `background-color` and horizontal `padding` on `code`, with the padding converted to em of the code size. `pre code` resets both, so code blocks keep their plain look.

## 2026-10-11: Figure styles

- **Syntax:** a `figures` object on block and custom styles with `style` (`lining`, `oldstyle`) and `spacing` (`proportional`, `tabular`), each `default` when omitted. An object keeps the two OpenType axes typed, so a theme cannot ask for lining and oldstyle at once, and layers merge it per key. A list of keywords would need a conflict check; raw feature tags would leak OpenType into themes for four values.
- **Shaping:** `Fonts::shape_with` takes the figures and sets `lnum`, `onum`, `pnum`, and `tnum` for Rustybuzz. Every place that shapes text in a style passes that style's figures: paragraphs, bands, heading and list numbers, contents page numbers, bibliography labels, drop capitals, code blocks, and the watermark. Inline code and footnote markers keep the font's defaults. With `default`, all four features are off, so existing output is byte-identical.
- **EPUB:** `font-variant-numeric` on every block style, `normal` on inline code.

## 2026-10-12: Smart punctuation

- **Rule:** straight quotes become the curly quotes of `lang`, “…” and ‘…’ in English, „…“ and ‚…‘ in German; `--` an en dash, `---` an em dash, `...` an ellipsis. Always on: a typesetter that leaves `"` straight in German prose is wrong by default, and a backslash keeps a single character straight, so no setting is needed.
- **Parsing:** pulldown-cmark's smart punctuation does the pairing, so escapes, code spans, code blocks, and link targets stay untouched. It emits English quotes; [`markdown.rs`](../src/markdown.rs) maps them to the document language. In German the closing single quote ‘ differs from the apostrophe ’, which CommonMark writes the same way. A paired opening quote is closed by the first later quote that may close, by the same flanking rule pulldown-cmark uses; every other one is an apostrophe.
- **Consequences:** `markdown::parse` takes the language. The terms of [index marks](#2026-10-12-index) are read from the source, so `punctuate` gives them the same treatment; front matter strings such as `title` are not Markdown and keep what the author typed. Documents with straight quotes or double hyphens change; page counts of the samples stayed the same.

## 2026-10-12: Callouts

- **Syntax:** GitHub alerts, `> [!NOTE]`, `TIP`, `IMPORTANT`, `WARNING`, `CAUTION`. pulldown-cmark 0.13 parses them as `BlockQuote(Some(kind))` with `Options::ENABLE_GFM`. In that version the flag changes nothing else (checked in `firstpass.rs`; footnote handling is a separate flag), and every sample without alerts rendered byte-identical. The parser keeps them as their own `Block::Callout`, so later work such as tagged PDF can tell them from quotations.
- **No custom title:** `> [!NOTE] Title` is a plain quotation in pulldown-cmark and on GitHub, so supporting it would mean a second parser for the marker line, and a document that uses it would differ in other tools. A theme can rename a label for a whole document. Revisit if documents need per-callout titles.
- **Unknown kinds:** a plain quotation that shows the marker text, as on GitHub. Failing would break portability for one typo class, and the text is visible in the output, so the typo is easy to see.
- **Theme:** `styles.callout` (text, spacing, `indent`) and `styles.callout-label` follow the existing block styles. `callouts.padding` and one `{ background, rule }` entry per kind carry the box. The bar reuses the table rule shape. A frame and rounded corners are not offered: `Item::Rect` is the only primitive, and a bar plus background covers the scholarly look. Labels are `labels.<lang>.note` and the other kinds, so `lang` and theme overrides work as for other generated text. The default design is light gray with a bar whose weight and darkness grow from notes to cautions, so kinds differ without color.
- **Layout:** the callout sets its label and blocks in a narrower frame, like a quotation, and each line records its box. The box is drawn when the page is placed, since the gaps between lines are only known then, from the first to the last line on the page. Padding above and below is added to the first and last line, so it takes part in page fitting. Where a callout continues on the next page, or goes on from the previous one, the box reaches its padding into the margin instead. This shows that the callout goes on without reserving space that a line would need.
- **Pagination:** the label is set like a heading, so it never ends a page. The text lines reuse the widow and orphan costs: no page ends after the first text line or before the last, a column does at a cost. Together with the paragraph rules this means a callout of fewer than four text lines is not split, and a longer one leaves at least two on each page.
- **Footnotes and tables:** notes have no box drawing, so a callout in a footnote is a plain quotation. Table cells cannot hold quotations. In columns the box takes the column width.
- **EPUB:** `<aside class="callout tip" epub:type="tip">` with the label as `p.callout-label`. `epub:type` is `tip` for tips, `warning` for warnings and cautions, and `notice` for the rest. The theme becomes CSS: text style and padding on `aside.callout`, background and a left border per kind.
- **Follow-ups:** a box that reaches into the margin may touch the footnote area when the gap is smaller than the padding; a `frame` rule if a theme needs one.

## 2026-10-12: Syntax highlighting

- **Behavior:** a fenced code block whose info string starts with a language is colored by token, in the PDF and the EPUB. A block without a language, with `text`, or with a language the highlighter does not know is set as before, without a warning. Indented blocks and inline code are never highlighted. Only the color changes: the line is shaped once with the `code-block` face and cut into runs of one color, so glyphs, advances, and line widths equal the plain block, and "code is never wrapped" is unchanged. This matters because Libertinus Mono has a regular face only.
- **Crate:** `syntect` 5.3 with its pure Rust `fancy-regex` backend (no Oniguruma, so no C toolchain), plus `two-face` 0.5 for the grammars. syntect parses with real Sublime grammars, so multi-line strings, nesting, and embedded languages work, and it is deterministic: same input, same scopes on every platform. Its default syntax set has 75 grammars but no TypeScript and no TOML, so `two-face`, bat's curated set of 213 grammars, supplies the syntaxes; syntect is still named directly for the parser. The set loads on the first highlighted block (about 50 ms in a release build) and costs nothing for documents without one.
- **Alternatives:** `synoptic` is pure Rust and smaller, but it matches regular expressions per line without a parser and ships rules for about 35 languages that its README says may be inconsistent. The tree-sitter highlighters (`tree-sitter-highlight`, `inkjet`, `syntastica`) need a C compiler. A hand-written tokenizer would have to be kept in step with every language it covers.
- **Size:** the release binary (fat LTO, stripped, macOS arm64) went from 9,233,840 to 11,647,344 bytes, 2.4 MB or 26% more. In a scratch crate with the same profile, `synoptic` added 1.27 MB and syntect with `two-face` added 2.45 MB over an empty program. The 959 KB grammar blob is less than half of that; the rest is the parser and regex engine, which any syntect set needs. Building the parser dependencies with `opt-level = "s"` saved only 150 KB and was not kept. Curating the languages would save about 0.5 MB at best (estimated from syntect's 75-grammar set, 413 KB against 959 KB), and `two-face` ships no sources to build a smaller set from, so all 213 grammars stay. Revisit if size matters more than coverage: syntect's own 75 grammars would save about 0.5 MB but lose TypeScript, TOML, Kotlin, Swift, and Dockerfile.
- **Languages:** the language is the first word of the info string, matched without case against a grammar name or file extension: `rust`, `ts`, `tsx`, `js`, `json`, `bash`, `sh`, `python`, `yaml`, `toml`, `html`, `css`, `scss`, `sql`, `c`, `cpp`, `cs`, `go`, `java`, `php`, `ruby`, `swift`, `kotlin`, `lua`, `xml`, `markdown`, `diff`, `dockerfile`, `makefile`, and the rest of bat's set. The aliases `shell`, `jsonc`, `json5`, `jsx`, `golang`, `objc`, and `csharp` are added where the set has no such name. The set has no PowerShell.
- **Categories:** scopes map to eleven categories: `comment`, `keyword`, `string`, `number`, `constant`, `type`, `function`, `operator`, `punctuation`, `tag`, and `attribute`. A scope is matched by prefix, the longest rule wins, and the innermost scope with any rule decides the token. So `punctuation.definition.string` is a string and `keyword.operator` an operator, while an identifier takes the category of an enclosing string or none. `tag` and `attribute` hold what would otherwise be miscolored: HTML tags and YAML and TOML keys, and attribute names, CSS properties, and annotations. Some grammars cannot be told apart by scope: the Rust grammar marks `let` and `u32` both as `storage.type`, so both are keywords.
- **Theme:** `highlight` is an object from category to color (`#rrggbb` or `$colors.x`), or `null` for no highlighting, which follows `watermark` and `inline.code.background` as the explicit off switch. Layers merge the object by category. An unknown category and a color that is not a color are errors, and a category left out keeps the `code-block` color. The Rust type is a `BTreeMap<Category, ...>`, so the category list lives in one enum.
- **Default palette:** keywords and tags `$colors.accent`, comments `$colors.muted`, strings `#2d6a4f`, numbers and constants `#8a4b08`, types and attributes `#1b6b73`, functions `#5a3d8a`, operators and punctuation `$colors.text`. Each is dark enough to read in grayscale, with comments the lightest. The scholarly look comes from using one accent and muted earth tones, and from leaving punctuation and identifiers black.
- **EPUB:** `<span class="hl-name">` around each colored stretch and one `pre .hl-name { color }` rule per category. Nothing is emitted when `highlight` is `null`.
- **Interfaces:** `Block::Code` gains `language`. The highlighter is the `highlight` module and the run splitting is `layout/highlighted.rs`.
- **Deferred:** an attribute on a fence to switch highlighting off for one block, line highlighting, and bold or italic tokens (the code font has a regular face only).

## 2026-10-12: Index

At the owner's request, outside the milestone plan. Authors mark terms in the text, and Druck sets a back-of-book index whose page numbers come from the final layout. Authors read [authoring](AUTHORING.md#index), theme authors [themes](THEMES.md#index).

- **Mark syntax:** `{^term}` marks its position, `[text]{^}` indexes the bracketed text and keeps it in the sentence, and `[text]{^term}` indexes it under another term. Braces are where Druck already puts its own syntax (placeholders, `{#label}`, `{.style}`), and `^` cannot start a placeholder name, so no document that rendered before changes meaning unless it holds a literal `{^`. `!` for sub-entries and `|see` and `|see also` follow makeindex, so LaTeX users recognize them; `[text]{^}` follows Pandoc's bracketed spans. An empty entry in a bracketed mark stands for the text, as in `[TeX]{^!history}`. Two levels of sub-entries are allowed, since the tree code costs the same for any depth and three levels cover real indexes. `siehe` and `siehe auch` are accepted beside `see`, as citations accept `S.`. Marks are read from the source text like citations, so Markdown inside them stays text.
- **Headings and table headers:** a mark in a heading is an error, like references, citations, and placeholders, since heading text feeds the contents, bookmarks, and running headers; the index lists the page of the text below instead. CommonMark reads a mark at the end of a heading as heading attributes, so that case gets the same message. A mark in a table header is an error like a footnote there, since the header repeats on every page and its anchor would have several pages.
- **Other errors:** a mark alone in its paragraph has no word to stand beside and is an error. A `see` reference to an entry that no mark makes is an error at the mark, since a dangling reference is a typo more often than not. Bracketed text holding a reference, citation, or footnote is an error, since the term must be plain text.
- **Placement:** as with the bibliography, `::: index` places the index, at most once, not inside `keep` or `bottom`, and allowed in `columns` and `full-width`. Without the directive the parser adds `Block::Index` at the end of a document with marks, after an added bibliography. A document without marks has no index, even with the directive.
- **Heading:** the `index` label ("Index", "Register") as an unnumbered level 1 heading, recorded in [structure](../src/layout/structure.rs) like the bibliography's, so it is in the contents, outline, and `{section}`. Unlike the bibliography's heading in layout today, it opens a chapter as the theme's `chapters` section sets, as [themes](THEMES.md#books) promises for every level 1 heading; in the book sample it starts on a recto page with the sink and ornament.
- **Anchors:** each mark gets an anchor, allocated after the structure walk so other anchor numbers do not change. Layout replaces `Inline::IndexMark` with a new `Inline::Anchor`, and [the paragraph composer](../src/layout/paragraph.rs) places the anchor where the fragment it belongs to starts: the word the mark follows directly, or else the next word. A mark after a word therefore stays on that word's page. Anchors leave line breaking alone, so documents without marks render byte for byte as before.
- **Settling:** the anchors of marks that show a page join the shown anchors, so the existing loop repeats layout until their page numbers agree. Every index entry is laid out in each pass from the numbers that pass assumes. Measured on 2026-10-12: the book and report samples settle in 2 passes, as before, and so does a 41-page document with 720 marks and the index placed before them; it renders in 0.22 s with a release build. A mark on a page without a number is an error at the mark, like a page reference to such a page.
- **Locators:** a page counts once per entry and links to the first mark on it. Pages are compared by the number they show: decimal and roman numbers are read back, and three or more pages in a row of the same numbering become a range, as numeric citations do. In a range the first and last number link, as in citations. A book whose parts restart the same numbering twice shows the same number for two pages, and they merge; the default theme continues the back matter instead.
- **Sorting:** one collation for English and German following DIN 5007-1: a table folds the Latin letters with diacritics to their base letters, ß to ss, æ and œ to ae and oe; case does not count; spaces and punctuation sort before letters, so the order is word by word. Ties go to the term without diacritics, then lowercase, then the characters. English dictionary order agrees with DIN 5007-1 on Latin letters, so the language changes only the words: the heading, "see", "see also", and the `symbols` group of terms that start with a digit or symbol, which comes first. A table instead of a Unicode collation crate keeps the dependency list as it is.
- **Layout:** in its own module, [`layout/index.rs`](../src/layout/index.rs), so later work such as tagged PDF can treat the index as its own kind of content. Letters in `index-letter` keep with the first entry, entries in `index-entry`, sub-entries in `index-sub-entry` indented by `index.level-indent`, later lines hanging by `index.hanging-indent`. An entry keeps with its first sub-entry and its last sub-entry with the one before, so no sub-entry starts a column alone. `index.columns` is 2 by default; the entries then form a two-column run below the full-width heading. An index inside `::: columns` takes those columns. Page numbers are links in `inline.link`, like citations.
- **EPUB:** marks become empty `span` elements with the anchor's id. The index file uses the EPUB Indexes vocabulary (`index`, `index-group`, `index-entry-list`, `index-entry`, `index-term`, `index-locator`, `index-xref-preferred`, `index-xref-related`). A reflowable book has no pages, so an entry links to its marks by the title of the heading each follows, each heading once; a footnote's marks follow the heading of the footnote's reference. One column, since readers lay out the page.

Not included: page ranges marked in the text (makeindex's `|(` and `|)`), sort keys that differ from the shown term (`sort@Term`), emphasis on main page numbers, several indexes, a "continued" line where an entry breaks across columns, and an `index` manifest property for the EPUB. These were left for when a document needs them.

## 2026-10-12: Tagged PDF and conformance

At the owner's request; the briefing had deferred tagged accessibility and PDF/A. Authors read [authoring](AUTHORING.md#accessibility-and-archiving).

- **Tagged by default:** every PDF has a structure tree, the document language, and its title in the metadata. Layout opens an element for each block and inline that carries meaning, in document order, and marks the page items that are its content with `Item::Begin` and `Item::End` ([`tags`](../src/tags.rs)). Unmarked content is an artifact, so running heads, page numbers, the watermark, rules, ornaments, scene breaks, crop marks, title page images, a table header repeated on a later page, and the continued marker of a footnote need no code of their own. Underlines, code backgrounds, and table rules are rectangles and always artifacts.
- **One mapping:** [`pdf::structure_type`](../src/pdf.rs) maps each `tags::Kind` to a standard type: Title (role-mapped to P), P, H1 to H6 with their text as title, L with Decimal or Disc numbering, LI, Lbl, LBody, BlockQuote, Code, Figure with the image's description as Alt, Caption, Table, TR, TH with column scope, TD with column spans, Note, Reference, Link, TOC, TOCI, and BibEntry inside a P. A new block becomes an element by opening a `Kind` around its lines with `Flow::open` and `Flow::close`, and gets its type in that one match.
- **Reading order:** elements are numbered as layout creates them, which is document order, so two columns, footnotes at the page foot, and future floats read in source order. Within an element, its content and the links and references begun inside it keep the order they are drawn in. A note follows its reference inside the paragraph, as the Well-Tagged PDF guidance and Typst do. A figure's Caption follows the Figure as a sibling, because a Figure's Alt hides its children from assistive technology; a table's Caption comes first, as it is set above the table.
- **Word breaks:** layout sets words apart without space characters, so text read from the structure ran words together. The writer now adds a space glyph without advance before a run that follows a gap on the same line, or that starts a new line of the same element after a line that does not end in a hyphen. Pages render identically; `pdftotext` output is unchanged. Hyphens at line ends stay hard hyphens.
- **Conformance:** the front matter setting `conformance` writes `document.conformance`, so it also works with `--set` and in themes. It takes `pdf/ua-1`, `pdf/a-2b`, `pdf/a-2u`, `pdf/a-2a`, or a list such as `[pdf/a-2a, pdf/ua-1]`; two PDF/A levels are an error. PDF/A-2 is the newest part krilla 0.8.2 combines with PDF/UA-1 (PDF/A-4 needs PDF 2.0, PDF/UA-1 at most 1.7). PDF/A-1 forbids transparency, which PNG images with alpha use; PDF/A-3 only adds embedded files, which Druck does not write. Level a needs tagging, which every file has; level u needs text for every glyph, which Druck's runs carry.
- **Validation:** krilla's validators run when the file is finished. Each error becomes a diagnostic and no PDF is written; an image without a description is reported at its line, through the figure's tag location. `check` writes the PDF and drops it, so it reports the same problems as `render`.
- **Dates:** PDF/A needs a creation date. Without an ISO `date` it is the build day, `SOURCE_DATE_EPOCH` or today, as for the EPUB. Files without a standard keep the old rule.
- **Title bar:** krilla 0.8.2 writes `DisplayDocTitle` only for PDF/UA, so viewers show the title instead of the file name only in PDF/UA files. The title is in every file's metadata. Writing the flag for all files needs a krilla change.
- **Not checked by krilla:** semantic requirements of PDF/UA and PDF/A level a, such as meaningful descriptions, a logical heading order, and color contrast. veraPDF was not available to check the files.

EPUB Accessibility 1.1 metadata is written to every package ([`accessibility`](../src/epub/accessibility.rs)): access modes, sufficient modes, features, the hazard `none`, and a summary in the book's language. The book claims `EPUB Accessibility 1.1 - WCAG 2.1 Level AA`, with `a11y:certifiedBy` Druck, only when every image has a description and every text color of the stylesheet has a contrast of at least 4.5:1 on white, inline code on its background. Running heads and the watermark are not in the book, so their styles do not count. Druck's own markup provides the rest: language on every document, headings, lists, tables with header scope, semantic notes, and text in `rem` and `em`. Content documents gained DPUB-ARIA roles beside their `epub:type`: `doc-chapter`, `doc-bibliography`, `doc-noteref`, `doc-footnote`, `doc-backlink`, `doc-toc`, and `doc-cover`; table headers gained `scope="col"`. Bibliography entries keep only `epub:type`, since `doc-biblioentry` is deprecated.

Features merged after this one map onto it as follows: a callout is a Div holding its label and its blocks as paragraphs; the index is an Index element after its heading, with an H2 per letter group and a P per entry; a margin note is a Note in the paragraph that references it. A margin note has no mark in the text, so [`Tree::add_margin_note`](../src/tags.rs) adds an empty mark for its element at the end of the referencing line, which places it after that line in the reading order. Sidenotes are footnotes and keep the footnote tagging.

Not included: soft hyphens at line ends, PDF/UA-2 and WTPDF, which need PDF 2.0, alternative text for theme images, which are decoration, and page navigation in the EPUB.

## 2026-10-12: Margin notes and sidenotes

At the owner's request, outside the milestone plan: Tufte-style notes beside the text. Authors read [authoring](AUTHORING.md#margin-notes), theme authors [themes](THEMES.md#margin-notes).

- **Syntax:** a margin note is a footnote whose definition starts with `{-}`: `[^label]` in the text and `[^label]: {-} Note text.` anywhere. `{-}` already means "unnumbered" for headings, and the pandoc-sidenote filter uses the same mark for unnumbered margin notes, so no new reference syntax was needed. All footnote rules apply: one reference per note, no reference inside a footnote, no definition without a reference. The parser keeps margin notes apart, as `Inline::MarginNote` and `Document::margin_notes`, so footnote numbers skip them and a missed match is a compile error rather than a wrong number.
- **Sidenotes:** `margin-notes.sidenotes: true` sets every numbered footnote in the margin instead of the bottom area, numbered as before and starting with its raised number. It is a theme setting, because it changes the page design and not the content.
- **Geometry:** the note column sits `margin-notes.gap` beyond the outer edge of the prose and is `margin-notes.width` wide. The outer side follows `page.margins.mirror`: right on odd pages and left on even ones, or always right without mirroring. Measuring from the prose edge puts the column into the free part of the frame when `page.text-width` is narrower, which is the Tufte layout, and into the page margin otherwise. A column that runs off the page is an error naming `margin-notes.width` and `gap`. A column that reaches into the frame while `page.wide` lists block kinds is an error too, since wide blocks would run under the notes; set `wide` to `[]` for a Tufte layout. Both are checked only when a document has a margin note or the theme sets sidenotes, so themes without notes are not constrained.
- **Text:** notes are set in the style `margin-notes.style` names, built-in or custom, at the column width, with `margin-notes.align` instead of the style's alignment. The default theme uses `footnote` and `left` in a 20mm column 3mm from the prose, which leaves 5mm to the edge of an A4 page with the default margins, because justified text in so narrow a column leaves wide gaps. A default custom style was tried instead, but it would show up in every theme's list of custom styles and in EPUB classes. Lists, quotations, and code in a note are scaled like those in footnotes.
- **Alignment and stacking:** a note's first baseline sits on the baseline of the line that holds its reference, in a table cell the line of the cell. Notes of a page are taken in the order of their wanted positions, top to bottom, ties in reading order. Each starts at its wanted position or `margin-notes.spacing` below the note above, whichever is lower. If the last runs past the bottom of the text area, the notes are moved up from the bottom, each `spacing` above the next, as far as needed. A note may therefore start above its reference, never below the frame.
- **Pagination:** with moving up allowed, a page's notes fit exactly when their heights and the spacing between them fit the text height. Notes are in reading order in the flow, so a page's notes are a contiguous range and the check is a difference of prefix sums, constant time per candidate page. A page whose notes do not fit is never a candidate. Before the search, every run of lines that cannot be broken is checked with the notes referenced in it; a note taller than the text area, or notes that cannot share a page with the lines they belong to, are errors naming the note's definition. Margin notes never use the bottom footnote area, so the footnote rules are unchanged.
- **Columns:** notes from both columns of a two-column section and from `full-width` blocks go to the same outer column, aligned with their line. Putting the first column's notes in the inner margin was rejected: inner margins of bound documents are narrow, and two stacks would tie the note check to the column split chosen during balancing.
- **Errors:** a margin note referenced inside a footnote is an error, as every reference inside a footnote is. A margin note in a table header row is an error, as footnotes are there.
- **Output:** `Output::asides` lists every placed note with its page, its box, what it is (margin note or sidenote by index), and where its reference is, so tagged PDF can mark notes as their own kind and keep them in reading order after their reference. The PDF writer draws them as ordinary items for now.
- **EPUB:** sidenotes are footnotes, written as before. A margin note has no mark to link from, so it becomes `<aside class="margin-note">` after the block that references it, set in the note style. Readers show it in place.

## 2026-10-12: Figure placement

At the owner's request, and against the briefing's exclusion of automatic floats. It stays a bounded placement: a block goes to the top or bottom of the page its anchor falls on or a later one, in document order, never past an explicit page break.

- **Syntax:** `placement=top`, `placement=bottom`, or `placement=here` in the attribute braces that already exist: after an image, `![Caption](chart.svg){#fig:chart placement=top}` or `{placement=top}`; at the end of a table caption, `: Caption {#tbl:x placement=bottom}`; and on the table directive, `::: table {placement=top}`, which also wraps a pipe table without a caption. `here` is the current behavior.
- **Theme default:** `placement.figure` and `placement.table`, both `here` in the default theme, so existing documents are unchanged.
- **Where:** placed blocks stand in the main text or in a `full-width` block of a column section. In lists, quotations, keep and bottom groups, and in a column of a two-column section, `placement=top` or `bottom` is an error, and the theme default does not apply. A table placed by the theme that holds footnote or margin note references stays in place; with an explicit attribute it is an error, because notes start on the page of their reference and are numbered in reading order.
- **Model:** layout takes a placed block's lines out of the flow and gives them to the composer as a unit with its anchor, the place in the flow where it stood. The text around it continues as if it were not there, so a paragraph after a placed figure keeps its first-line indent, as LaTeX floats do.
- **Search:** a state of the page search is now the break position, the footnote lines placed, and the placed blocks set. A page may set the next blocks in order whose anchor lies before its end. Blocks set on one page go top first, then bottom: a bottom block followed by a top block in document order ends the page's choice, so the top one waits for the next page. Each block set takes its height and the caption style's `space-after` from the text area, at the top or above the footnote area. Each page a block waits after its anchor's page costs 10 000, about as much as a page holding only continued footnotes, so a block goes to its anchor's page whenever it fits there and to the next page otherwise.
- **Flushing:** a position after a forced break (page break, chapter, part, bottom group) or the document's end can only be left once every block anchored before it is set. Blocks still waiting there go on pages of their own, stacked from the top, after the last text page before the break, as LaTeX's `\clearpage` does. Such a page costs nothing beyond the waiting, since the last text page before a break may be short anyway. A page of placed blocks amid the text costs 100 000, so it is a last resort for blocks that cannot share a page with text. At first it cost 10 000 like a page of waiting, and the sample then set two blocks on a page of their own rather than wait a page beside text. Since each block fits a page on its own, the search always finds pages.
- **Columns:** a block in a `full-width` block of a column section goes to the top or bottom of the page across both columns. A `full-width` block that holds only placed blocks does not split the column section, so the columns stay balanced across the anchor. Column regions take what the placed blocks leave, so balancing is unchanged. Column-width placement inside a column was left out: the column split is chosen during balancing, and a block at the top of a column would make column heights depend on which column the anchor falls in.
- **Fit:** a placed figure shrinks to the text height with its caption, as before. A placed table must fit on one page and is reported at the table otherwise, since continuing pages would mean a queue of partial floats.
- **References:** figure and table anchors move with their lines, so cross-references and page references name the page where the block lands; settling handles the page numbers as before.
- **Order:** placed blocks never pass each other. A block left in place may come before a placed one that waits, so "Figure 3" can precede "Figure 2" on paper; numbers stay in document order. A theme that places all figures avoids this.
- **EPUB:** placement is ignored; blocks stay where they are written.
- **Sample:** [margins](../samples/margins.md) shows margin notes, top and bottom placement, a block that waits for the next page, and a placed figure in a column section; with [its theme](../samples/themes/margins.json) it is a Tufte layout with sidenotes and figures placed at the top by default.
- **Measurements:** on 2026-10-12, `samples/en.md` body × 52 (105 pages) took 0.41 to 0.42 s user time and 45 MB with and without these changes, and byte-identical output. The same input with a margin note and a figure placed at the top in every copy (118 pages) took 0.49 s and 52 MB. Documents without placed blocks keep one state per footnote position, so their search is unchanged.

## 2026-10-17: Typesetting rules

At the owner's request: Druck should produce book-quality pages from rules and theme settings rather than manual fixes. LaTeX's defaults allow both cases that started this: a one-line paragraph alone at the top of a page, and a heading with two lines below it at the foot of a page.

- **Model:** every rule is a cost in the existing searches, weighed against short pages, so the search can break one where keeping it costs more. Only the existing limits stay hard. Costs are measured in `SHORT`, the cost of a page one line short (1000, squared per line), so the rules compare.
- **One-line paragraphs:** a page break before or after a one-line paragraph in the main text costs `ONE_LINER`, as much as a page four lines short. Lists and footnotes are exempt, since one-line items are normal there.
- **Lines under a heading:** two lines stay the hard minimum. Each line fewer than `typesetting.heading-lines` (default 3) costs `HEADING_LINE`, a page one line short. The count runs across blocks after the heading.
- **Chapter endings:** a page ending at a forced break or the document end that continues a chapter costs `SHORT` × missing² when it holds fewer body lines than `typesetting.chapter-last-lines` (default 5).
- **Facing pages:** in duplex, a right-hand page costs `SHORT` × difference² against the shortfall of the left-hand page it faces, unless either may be short. A search state now also holds the page parity and, after a left-hand page, its shortfall in half lines. Blank pages before recto lines are predicted as `render` inserts them. Without duplex the states are as before.
- **Paragraphs:** more than `typesetting.hyphens-in-a-row` (default 2) hyphenated lines in a row cost 10⁷ demerits, finite so a paragraph always breaks. The best node per break is kept per fitness class and hyphen run, so the limit is exact. A last line narrower than max(2 × the style's first-line indent, 3 em) costs 5000 demerits, like a hyphen before the last line.
- **Hyphen fragments:** English keeps 2 letters before and 3 after a break, German now 3 and 3, through `hypher::hyphenate_bounded`. `typesetting.hyphen-fragments` overrides both. The owner chose longer German fragments; LaTeX's German default is 2 and 2. On the owner's 200-page German manuscript, 3 and 3 gave 145 loose lines, 2 and 2 gave 40.
- **No-break spaces:** a one-pass rewrite of author text (`src/spacing.rs`) joins single-letter abbreviations, numbers and a fixed list of units, and reference abbreviations and their numbers with U+00A0. Code, link destinations, and spaces across inline boundaries are untouched. `typesetting.no-break-spaces` and the front matter key turn it off. U+00A0 stays inside a word, so it neither breaks nor stretches.
- **Warnings:** layout reports each rule it breaks with the page as numbered and, where one block is concerned, its source location. `render` prints the count, `check` lists them. Pages count as short above one line and spreads as uneven above half a line, since smaller differences come from headings that are not on the line grid.
- **Theme:** only the four values above and the switch are settings, in one `typesetting` section. The costs stay constants.
- **Effect:** on the owner's manuscript (A5-like, German, recto chapters), both cases are fixed and the book grows from 201 to 207 pages: the one-liner rule adds 4 and the spread rule 2, since chapters start on odd pages. Pages more than a line short rose from 22 to 33 (32.6 to 46.5 lines in total), and no one-line paragraph stands at a page edge any more. Still reported: 7 headings with two lines below them, 3 short chapter endings, and 20 uneven spreads. Rendering took 0.65 s user time and 62 MB, against 0.59 s and 52 MB before.
- **Left out:** microtypography (font expansion, protrusion of ragged text), a baseline grid, and river detection.

## 2026-11-03: Desktop app

The product direction here is replaced by [the writing-first direction](#2026-11-05-writing-first-direction). Its toolkit, text-as-truth, and theme decisions still stand.

Settled with the owner in an interview. The contract is [the desktop briefing](DESKTOP_BRIEFING.md).

- **Toolkit:** gpui through the crates `gpui-pre` and `gpui-pre-platform`, pinned at exactly 0.3.11, since Rust-only rules out Tauri and webviews. They are snapshots of zed@f816e1f published by one person, chosen because they have the editor APIs and AccessKit without Zed git sources. `src/` of gpui matches that commit apart from small `action.rs` changes; the platform crates beyond `gpui_platform` were not diffed. Zed's `editor` crate is GPL-3.0, so its code cannot be used in MIT Druck, and the rich text editor is our own.
- **Text is the truth:** a block model that serializes on save would reformat files that agents and other editors also change. This needs a source-preserving layer, since the parser records only block start locations and cannot reproduce source.
- **Spike before milestone 10:** the owner chose to retire the editor risk first. This replaces "next: milestone 10" until the spike and engine amendment are done.
- **Comments are accepted:** "Embedded HTML is not rendered" still holds. Comments were errors, which forced `build.sh` to strip them and pointed diagnostics at a generated file.
- **Underline and strikethrough are added:** this reverses "not supported" in authoring, at the owner's request, though they are weak typography.
- **Overrides are named styles:** the mockup's per-block inline styles are not adopted, because the briefing rules out cascading rules and keeps Markdown additions small.
- **Theme forks copy assets:** the briefing forbids parent chains, so a project theme is a self-contained copy. Common theme updates do not flow in.
- **Chapter order is a manifest list:** chosen over file name order, so an agent adding a chapter must also edit `druck.yaml`; until then it is flagged and excluded.
- **Agent panel spawns the CLI:** the Agent SDK is not a Rust library. The login policy question is open until Anthropic confirms it.

Unresolved: Anthropic's answer on subscription logins, and gpui's accessibility on each platform.

## 2026-11-03: Desktop spike results

What the [spike](milestones/14-desktop-spike.md) settled. Measurements are in [progress](PROGRESS.md#verification).

- **Pages are rasterized on the CPU:** with skrifa, tiny-skia, and resvg, because gpui's `Font` has no variation coordinates, finds fonts by family name rather than by file, and its glyph sprites cannot rotate, which the watermark needs. See `desktop/src/preview/raster.rs`.
- **The galley finds the theme's body font by name:** the engine's bytes for the family are given to gpui with `add_fonts`, and its name is read from the file. Sizes are the theme's points at 1.4 logical pixels each, because the galley is for reading on screen. Headings scale from the body. Other theme properties do not reach the galley yet.
- **Text buffer and block rules:** the buffer holds the file's bytes and every change is an edit to them. A blank line ends a block. Each source line is one visual line, soft wrapped. Each `:::` line is its own block. Inline markers reveal when the caret touches their span's edges, and line markers when the caret is on their line.
- **Theme edits splice text:** one member changes in the file's text instead of re-serializing, so a partial theme stays partial and keeps its formatting. A project theme is `themes/<name>/theme.json`, and wins over a common one of the same name.
- **Theme selection:** `--theme`, else the file the document's `theme:` names, resolved by the engine, so the inspector and the pages always use the same file.
- **Refresh:** pages are requested 150 ms after the last edit, and the running layout finishes before the newest text starts, because a layout cannot be stopped. Text stays in memory until Cmd+S. Layout keeps gpui's default background priority, since typing did not suffer from it on an 8 performance core M1 Pro. Lower it if a smaller machine shows otherwise.
- **Shape plans are cached per face, script, language, and features:** rustybuzz built one on every call. The worst case of pause to painted pages was 1.2 s without the cache. With it, a 203-page render went from 0.67 s to 0.39 s, and PDFs stayed byte-identical because the plan's inputs are the ones `rustybuzz::shape` uses.
- **Agent protocol:** `claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages --permission-prompt-tool stdio`, plus `--model` and `--resume` when set. `--permission-prompt-tool stdio` works but has no entry of its own in `--help`, so it is the most likely to change; `2.1.317` names the flag only in passing in another option's text. The panel keeps default tools and settings, so project `CLAUDE.md`, skills, and MCP behave as in a terminal, at a cost of about 33,700 cached tokens on a bare call.
- **What phase 2 needs:** source mapping (block id and source offsets on text items, a block-to-range table, and the chapter file), exposing the citation scanner the source layer now reaches through `druck::bibliography::syntax`, paragraph caches beside the shape plan cache, font reuse as a supported call rather than `load_fonts` and `typeset_with`, and no process-wide effects such as `show_relative_to`.
- **Known limits:** replacing a font file under the same path is not detected while the app runs, since loaded fonts are reused while the configuration asks for the same files. `prepare` decodes images again on each refresh. The galley uses Georgia until a document's first successful layout, which for a corpus chapter with comments means until milestone 15. macOS stops painting a window that is fully covered, so measurements float the window, see `desktop/src/gate.rs`.

## 2026-11-03: Comments, inline marks, and placeholders

The language half of [milestone 15](milestones/15-engine-amendment.md). It reverses "strikethrough is not supported" and lets comments through, as the [desktop decisions](#2026-11-03-desktop-app) accepted.

- **Comments are cut out before parsing:** `<!-- -->` is removed from the text, found outside code blocks and spans (one parse of the source finds those, only when it contains `<!--`). A comment on a line of its own leaves a blank line, so it separates blocks as a blank line does, and the paragraph sequence and first-line indent behave as if it were removed. Text on both sides of an inline comment joins. A table maps offsets back to the written text, so diagnostics, code line numbers, and todo locations stay right. An unterminated comment is an error at its start and cuts the rest. Comments pulldown-cmark would see as HTML blocks or inline HTML never reach it, so `<!-- c --> text` on one line is not swallowed as one HTML block. Other raw HTML is still an error.
- **One tilde meaning:** with strikethrough and subscript enabled, pulldown-cmark 0.13.4 reads `~~x~~` as strikethrough and `~x~` as subscript, and pairs delimiter runs only of equal length. Both pair only at word edges, so `H~2~O` stays text there. The parser pairs those from the source: the same character closes within the same word, with letters, digits, and a few signs between. This is Pandoc's rule without its escaped spaces.
- **Superscript is parsed here, not by pulldown-cmark:** its `^` pairing would match the carets of two index marks on a line, as in `{^a} and {^b}`, and turn the text between them into a superscript. A caret after `{` or `[` is never an opener. Existing documents are unchanged: none of the owner's samples or the ttp404 manuscript holds a tilde, and every caret there starts an index mark or a footnote.
- **Underline and small capitals are `[x]{.underline}` and `[x]{.smallcaps}`:** read from the source like index spans, so the brackets hold formatting but no brackets. Any other `[x]{.name}` is what it was. Inline custom styles are not added. `InlineStyle` gained `strike`, `underline`, `superscript`, `subscript`, and `todo`; `small_caps` is shared with the chapter lead-in.
- **Scripts are scaled and shifted, not `sups` and `subs`:** `inline.superscript` has `size` and `raise`, `inline.subscript` has `size` and `lower`, as `inline.footnote-marker` does, since lengths cannot be negative. Font features would need a feature argument in every shaping call and a fallback for fonts without them, which is the scaled path anyway, and coverage of `sups` varies between fonts.
- **Lines:** underline and strikethrough are rectangles like the link underline, in the text color, with the font's own position and thickness (`Metrics` gained the strikeout position). A line runs on across the spaces between marked fragments, and they are decoration in the tagged PDF.
- **Todos:** `{todo: text}` parses to ordinary text "TODO: text" with `InlineStyle::todo`, set in `inline.todo` (`color`, `background`). Layout, hyphenation, and the EPUB need no new inline kind; statistics skip text with the flag. `Document::todos` lists each with its location and text, in source order, for a final export. The text is read raw to the first `}`, so markup in it shows as written and the events inside it are ignored. A `{todo}` without a colon is still an unknown placeholder.
- **EPUB:** `<s>`, `<sup>`, `<sub>`, `<u>`, `<span class="small-caps">`, and `<span class="todo">`. The class of the chapter lead-in is renamed from `lead-in` to `small-caps`, and its rule is always written, since both are the same style. Every stylesheet gains the rules for `sup`, `sub`, and `.todo`.
- **Limits:** a heading takes a trailing `{...}` as its attributes, so `{.underline}` at the end of a heading is read as its style and a `{todo: x}` there is an error; a todo in a heading is an error anyway. Small capitals are never hyphenated, as before.

## 2026-11-03: Project folders and named exports

A folder with a `druck.yaml` is a document. The manifest deserializes into `FrontMatter`, the type front matter already uses, with `chapters` and `exports` added to it. Serde cannot combine `flatten` with `deny_unknown_fields`, and a second manifest type would parse the settings twice. Front matter and `--set` reject the two project fields with an error, so a single file behaves as before.

Chapters are joined into one text, with a blank line between them, and parsed once. Labels, footnotes, and citations are therefore book-wide with no extra code, and blocks may span chapters as in one document.

Locations are mapped where diagnostics are made. `Source::Chapters` holds a table of each chapter's first joined line, and `Diagnostic::at` turns a joined line into the chapter's `Source::Document` and real line, so no `Location` carries a file. Messages naming an earlier line use `Source::line_name`. `Diagnostic::position` keeps the line as parsed, so errors still sort in reading order across chapters. A diagnostic without a line, such as a page ending short, names the manifest. A line in the blank join counts as the last line of the chapter before it.

The manifest is the document for origins: its paths resolve against its folder through `Origin::Document`, with no rebasing. Paths in chapters, such as images, resolve against the project folder too, whichever folder the chapter is in, because the chapters are one document with one base. A Markdown preview of a single chapter in another tool therefore does not find them. Changing this needs a per-image base.

Unlisted Markdown files are found recursively below `manuscript/`, which took a few lines. A chapter listed twice is an error, because it would otherwise silently repeat text.

Theme lookup is one function, `theme_path::locate`, which the desktop library now calls. A bare name means `themes/<name>/theme.json` in the project, then an exact local file as before, then the shared directory.

An export is a fixed set of fields. Its `theme` and `draft` replace the manifest's before configuration resolves, so `--theme` and `--set` still win and errors still name the manifest. `final` only gates on placeholders and does not imply `draft: false`: a document with `draft: true` needs `draft: false` in the export too. `--export` misuse exits with 2 through clap, as other usage errors do. Consequence: `--crop-marks` cannot turn off marks an export sets, since the flag has no negative form.

There is no JSON schema for `druck.yaml`, as front matter has none. The published schema covers themes only.

## 2026-11-03: Release acceptance: limits, desktop stack, desktop CI

- **Performance limits:** a release build must lay out and write 10 pages in 0.25 s and 64 MB, 50 pages in 0.6 s and 96 MB, and 100 pages in 1 s and 128 MB, measured as in [progress](PROGRESS.md#verification) on a machine like the M1 Pro. They replace the proposed 2 s and 150 MB. The worst 100-page input measured 0.29 s and 76 MB (`samples/report.md` body × 12 with citations, an index, and highlighted code), so the limits leave about three times the time and 1.7 times the memory for slower CI machines and other allocators. A 203-page German novel took 0.38 s and 59 MB. Memory has a fixed part of about 18 MB with highlighted code, because the syntax set loads on first use (9 MB without code). Only macOS was measured; the limits are not enforced by a test, since timings on shared runners are not stable.
- **Layout thread in the desktop app:** the executor's background threads are libdispatch workers with 512 KB of stack. Deserializing a theme alone needs about 450 KB in a debug build, because the derived deserializers for `Theme` (11 KB) and `Styles<BlockStyle>` (7 KB) keep many large locals and each level adds six wrapper frames, so debug builds of `druck-desktop` overflowed the stack and died with SIGILL before a window opened. The depth is fixed by the schema, not by the input, so the need is bounded but not small. The fix is not to depend on the caller's stack: `Typesetter::run` lays out on a scoped thread with 16 MB of stack. Reducing the frames instead would mean reshaping the theme types for a debug-only cost. The CLI runs on the main thread (8 MB) and the integration branch shares the unfixed code. Other desktop paths that resolve a theme (`document.rs`, the theme file validator) run on the main thread and are unchanged.
- **Desktop CI:** one macOS job builds, tests, and lints `druck-desktop`, checks its format and its notices. The existing jobs are unchanged because the desktop crate is not a default member. `python3 scripts/notices.py --desktop` writes `desktop/THIRD_PARTY_NOTICES.md` for the crates the app links on macOS (352, against 140 for the CLI). 41 of them declare a license but ship no license file, and `option-ext` is MPL-2.0; both need the owner before the app is distributed.

## 2026-11-03: Phase 2 step 1, choices beyond the design

The facade follows [the design](PHASE_2_API.md). These go beyond it or differ from it.

- **`Configured::config_json` returns a pretty `String`, not a `Value`.** `serde_json` is built without `preserve_order`, so a `Value` sorts its keys, and `druck check --print-config` would change its output. Turning the feature on would also change the order in every theme merge, so it is not worth the risk.
- **`Rendered` has a `parts` field:** pages of a PDF, content documents of an EPUB. The CLI's `wrote N sections` needs the EPUB count, and `Layout` has no public pages yet. The destructuring in the design's summary needs `..`.
- **`Renderer::render` returns configuration warnings too.** The design lists only layout and export warnings there, but `render` has no `Configured` to read the others from, so they would be lost.
- **`ExportSettings::print_conflict`:** the diagnostic for an export whose print settings do not suit an EPUB. It needs the manifest location of the export, which the CLI has no way to know.
- **`Configured::check_final(name)` instead of `Configured::todos`.** Parsing during `configure` would move a parse error ahead of the configuration warnings on stderr, and the CLI cannot build a diagnostic at a joined line. Step 3 can revisit this when the printing moves.
- **`tests/acceptance.rs` moved to `src/acceptance_tests.rs`.** It reads `page::Output` and needs the private `pipeline`. Gating an integration test on the `editor` feature would skip it in a plain `cargo test`. Step 5 can move it back once `Layout` is public.
- **Items only the desktop reads carry `#[cfg_attr(not(feature = "editor"), allow(dead_code))]`,** narrowly rather than for the whole crate. CI gains a `cargo clippy --all-features` line, because that build is the one that finds code nothing uses.
- **`druck-desktop` depends on `druck` with `default-features = false`,** so the app's build does not compile the CLI's feature. It has its own `clap`.

## 2026-11-03: Phase 2 step 2, choices beyond the design

The resolver and renderer follow [the design](PHASE_2_API.md). These go beyond it or differ from it.

- **`ResolveError::NotFound` carries the resolver's own message.** The engine prints it in `cannot read file: {e}`, so `No such file or directory (os error 2)` stays as it was on every platform. The design's unit variant would have changed the CLI's stderr.
- **`Bytes` wraps `krilla::Data`, not `Arc<[u8]>`.** krilla wants a `Data`, which is an `Arc` over anything that gives bytes, so a file read into a `Vec` and a bundled `&'static [u8]` go through without a copy. `MemoryResolver::insert` still takes `impl Into<Arc<[u8]>>`.
- **The content hash is std's SipHash 1-3 with a key of this process.** It measures 3.4 GB/s here, so no hash crate is needed. The key means nobody can prepare a file offline that collides with another. Hashes never reach output. A bundled file hashes its path, since its path fixes its contents, so bundled fonts cost no hashing per job.
- **A theme file is read through `Origin::Theme(its folder)`,** whatever selected it, so a memory resolver holds a theme and its assets under one origin. Nothing else is rebased.
- **`ThemeSelection::Name` has a `base`.** A bare name also means an exact local file, as before, between the project and the shared folder, and that needs a folder. `ThemeSelection::asked` makes the `--theme` choice between `Name` and `File`.
- **`Overrides::parse` and `from_json` take the working directory.** Relative paths in overrides resolve against `Origin::WorkingDir`, and the design's signature had nowhere to get it from.
- **`EpubOptions::modified`.** `dcterms:modified` depended on whether `SOURCE_DATE_EPOCH` is set, which the core may no longer read. The caller passes the day.
- **`resource.rs` holds what touches the machine,** for both clients: `FileResolver`, `FileResolver::shared_themes`, `source_date`, and `clock_date`. The CLI and the desktop would otherwise each copy them.
- **`InstalledFace` is `{ path, index, weight, italic }`.** `FileResolver::installed` applies the fontdb rules (the width closest to normal, italic before oblique, first path wins). The engine reads the files through the resolver and expands variable ones to their instances, so a resolver of another kind supplies the same thing. The lookup stays in the layout stage, because `check --print-config` and an EPUB never needed installed fonts.
- **A face's file name lives in the job's `FontSet`, not in `Face`.** The same bytes can sit under another name in the next job, and a message must name this job's file. Section 5 of the design has `Face::file`; step 4 should take it from the set.
- **Reuse is tracked by job number.** Each job takes the next number and, when it starts, drops what neither of the two jobs before it used. That leaves the same table as the design's "when a job ends" without a `Drop` that has to know what ended. With three or more themes alternating, faces get rebuilt: correct, without the gain.
- **An image that would pass the 256 MB budget is used and not kept.** Others are not evicted for it. The cost is the file plus 4 bytes per pixel, since krilla keeps decoded samples.
- **Configuration reads every resource it names,** including theme images no layout shows, because that replaces the existence check. `check --print-config` now reads fonts.
- **Desktop:** the painter is kept while the `FontSet` compares equal, which holds when the renderer returned the same faces, so a replaced font makes a new painter and new page images. `FontRequest` is gone. The galley's fonts are copied once per content hash for `add_fonts`, which needs `'static` or owned bytes. `Timing::fonts` is gone, because `layout` now covers it.

CLI behaviour that changes, in cases outside the guard:

- An invalid `SOURCE_DATE_EPOCH` fails first, including for `check --print-config`, which did not read it before. The caller must read it to build an `Input`.
- A bad `--set` is reported before a bad front matter when a document has both, since overrides are parsed to build the input.
- A font, image, or bibliography that exists and cannot be read now fails while configuring, as `cannot read {resource}: {reason}` with its property, rather than later with a message per kind.
- An image path naming a folder says `image not found` rather than `cannot read image ... Is a directory`, because a folder is not a file for every other resource either.
- Document image paths are normalized without touching the filesystem, as every other resource path already was, so `link/../x.png` through a symbolic link resolves differently.

Where the design was wrong or contradicts itself:

- Section 10 asks for a test that "the renderer holds no face after a job whose layout was dropped". Section 4's two-job rule keeps those faces, and the desktop needs that between refreshes. Section 4 is implemented, with a test that faces go after two jobs that do not use them and that a held layout keeps its own.
- Section 4's "microseconds per face" holds: about 8 microseconds per Libertinus face and about 30 per Fira Code face, 230 microseconds for the corpus's 11 faces. A render now does it about three times per face (once to key the face, once for shaping, once for the PDF writer's space glyph), which does not show next to a layout of 275 ms.

Temporary until later steps: `editor::engine::into_parts`, the PDF writer's per-run shaping of a space, `show_relative_to`, and `shown`.

## 2026-11-04: Phase 2 step 3, choices beyond the design

Diagnostics and cancellation follow [the design](PHASE_2_API.md). These go beyond it or differ from it.

- **The engine's input type is `InputSource`, and the public `diagnostic::Source` is another enum.** Rust has no private enum variants, and `Source::Chapters` must not be public. `InputSource` keeps the chapter map and the overrides' working directory, which decide origins. A `Diagnostic` built from it holds the public source and privately the chapter map, so `at` still turns a joined line into the real file. The `editor` feature re-exports `InputSource` for the theme validator.
- **`Diagnostic` has a public `resource: Option<Resource>`.** The contract asks for resource identifiers with their origins, and the design's struct had nowhere to put them. Every read, decode, and embed failure sets it.
- **`Diagnostic::message` holds absolute paths, and `display_from` shortens the ones it knows about.** Those are the resource's whole phrase, the project's chapter paths, and one named file (the bibliography in a missing-key message). A path shortened only by textual prefix would also change a sibling folder such as `/work/book two` for base `/work/book`. A message that repeats a known path in user text gets it shortened on a terminal too, and the field stays exact.
- **`Diagnostic::new` and `warning` are crate-private; `Diagnostic::error(message)` and `diagnostic::path_from` are public.** The CLI and the app report their own failures and print their own paths with them.
- **`Location` is the parser's `document::Location`, re-exported.** Two identical structs would need converting.
- **`Layout::pdf` takes a `&Cancel`.** The design has `pdf(&self, options)` and also a check per page. A layout outlives its job by design, so it cannot hold the job's borrowed token, and an export started later should not stop because the layout's old job was superseded. `Renderer::render` passes its one token to both.
- **Check points beyond the design:** each resource read in `configure`, each citation, each section of both EPUB rendering passes, and each document added to the EPUB. Citations and EPUB sections are short on the corpus, but the loops grow with the document and a check costs one relaxed load. No check sits inside `markdown::parse`, `Bibliography::parse`, or `citations.finish`.
- **`Flow::blocks` returns early when cancelled rather than returning a `Result`.** Blocks recurse through about ten callers that push errors into `Flow::errors` and carry on, so this keeps their shape. A caller may see fewer lines than it set, which an empty cell or note already means. `Pass::content` looks at the token before anything reads those lines. A test stops a document of nested blocks at every block, and all samples were swept this way once.
- **The token is checked again after `Document::finish`.** Finishing cannot be stopped and takes about 6 ms of the corpus's 104 ms export, so a cancel during it returns `Cancelled` instead of bytes.
- **The renderer counts a job when it ends, not when it starts, and not if it was cancelled.** Otherwise two cancelled jobs in a row would age the caches and the next layout would decode and build everything again. A job that only configures is no longer counted, which is the same for what it holds, since it uses no faces.
- **`CheckPoint` is an argument of `check` only so tests can name a place.** In a test build a `Cancel` can hold a probe that cancels at the nth visit of one check point and counts visits, which shows where a job stopped. Other builds have no probe and ignore the argument.
- **`bands::Band::draw` returns `Box<Diagnostic>`** for its one error, since the larger struct trips `clippy::result_large_err`.

This supersedes the [spike's](#2026-11-03-desktop-spike-results) "the running layout finishes before the newest text starts". The desktop's `Scheduler` cancels the running layout when a request arrives and starts the waiting request when that layout returns, so layouts still never overlap and the typesetter's lock stays uncontended. The 150 ms pause stays.

Measured on the corpus in release builds. Cancel to `Cancelled` during layout: median 1.3 ms, maximum 19.4 ms, over 88 cancels spread across a layout. The maximum falls before the first block, in parsing and citations, and 50 ms into a layout the maximum is 1.1 ms. Book layout: median 267.6 ms against 268.8 ms before, 80 samples each, so the check points do not show. `DRUCK_GATE`, last key just after a layout started to painted pages: median 467 ms against 612 ms for the base build, two runs of 30 samples each.

Already done before this step: the design says `pipeline::load` prints, but steps 1 and 2 had moved all printing to `main.rs`. This step removed the last process-wide state, `show_relative_to` and `shown`.

Temporary until later steps: `editor::engine::into_parts` and the PDF writer's per-run shaping of a space.

Where the design was wrong or contradicts itself:

- `Layout::pdf(&self, options)` in sections 2 and 5 against section 8's check per page, as above.
- Section 7 lists `pipeline::load` and `Job::open_project` as printing in the engine. Neither does any more.

## 2026-11-04: Phase 2 step 4, choices beyond the design

The layout result and source map follow [the design](PHASE_2_API.md). These go beyond it or differ from it.

- **A text item holds a `TextId`, and `SourceMap` holds the rest.** The design has `source: Option<TextSource>` on each item, with an `Arc<[Span]>`, and estimates 25,000 runs because it takes a run for a line fragment. A run is a fragment of a word, and the corpus has 39,645, so an allocation per run would not fit the estimate. A `TextId` is 4 bytes that fit in the padding of `Item::Text`, so an item stays 112 bytes with or without a map. A text is 8 bytes in the map, a place 8 more, and only runs of several pieces use more. `SourceMap::text` gives the design's `TextSource` for inspection.
- **`TextId`s follow the written order.** The map sorts its texts by offset when a layout ends and renumbers the items. So `locate` is a binary search over all text and needs no table per block, and a table caption, written outside its table's range, is found like anything else. The design's "innermost block" decides only one case: a place in the syntax that starts a block goes to that block's first text.
- **`position` is exact, and `nearest` is what a click wants.** `position` returns `None` for a byte the engine inserted, as the design says. The design also says a click on generated text yields the block's start. That is `nearest`, which does better where it can: inserted text records the place it stands for, so a click on a dash goes to its `--`, and a footnote marker or citation goes to where the word before it ends. Only text with no written text before it in its paragraph, such as a heading's number, goes to the block's start.
- **`Span` in the map is 8 bytes and private; the public `Span` is built on request.** The public one has the design's fields, with `source` as a `SourcePos`.
- **`Layout::hit`, `position_at`, and `locate` are in the engine.** The design leaves the hit test to the painter, but both directions then need the run's glyphs in the same way in every client. `Located::rect` is as tall as the run's size around its baseline, not the ink, since a face's metrics would need a parse.
- **`Inline::Text` gets a `source` field.** A side table would have to follow layout's edits of content: lead-ins, drop capitals, typed numbers, and resolved citations. With the field, those edits slice it. Blocks do use a side table, `Document::sources`, keyed by location and kind, because they already carry a location and adding a field to all 18 variants changes more.
- **Spans count in the document's whole text until the map is built.** A span can then cross nothing it should not: a comment splits it in the parser, and the chapter comes from the offset. `ChapterMap` gains each chapter's byte start and what its file has before that, a byte order mark. `ChapterId` is `u32`, since a `u16` could wrap.
- **Running heads have no `TextId`.** The design lists them as generated text of a block. They are drawn after the pages and name a section, not a block, and a click on one has no clear target. Title blocks, page numbers, and watermarks have none either.
- **`Structure` is the engine's `tags::Mark`, re-exported, with an opaque `StructureId`.** The design reduces it to a small enum, which would mean a second type and a conversion at every `Item::Begin` for values a painter skips. The element's kind stays private.
- **`Picture::pixels` gives `Raster { width, height }`, not RGBA.** The engine keeps krilla's image and does not hold decoded samples of its own. Holding them for painters would double the memory of every image. `Picture::source` gives the file, as the desktop already reads it. `usvg::Tree` stays exposed for SVG, as the design has it.
- **`Face::space` is a cmap lookup done once when a face is built,** so the PDF writer holds no shaper and `Layout::pdf` parses nothing. The bytes are unchanged on all samples and the corpus.
- **`Layout` holds copies of the metadata, language, conformance, day, and source** instead of `Arc`s of the configuration and the document's text, so a layout keeps what it needs and not the manuscript.
- **There is no `Layout::epub`.** An EPUB has no pages to reuse: readers set the text. `Configured::epub` stays where it is.
- **`SourceMap::memory` is `#[doc(hidden)]`,** there so the measurement can run through the public crate.

The desktop's painter moved to `druck::output` in this step, since the click needs the layout and `into_parts` could then go. It takes `Arc<Face>`s from the layout and is kept while they are the same objects.

The desktop's click sync: a click on a page moves the galley caret to that text, and in Split gives the galley focus, and a moved caret shows a 2 px accent line on the page and scrolls the page to it if it is off screen. Pages set from an older revision of the text have other offsets, so a click on them does nothing and a caret move waits for the next layout. The app always asks for a map. The line's look has not been reviewed by the owner.

Measured on the corpus in release builds, through the public crate, from a scratch project of the book with its 78 comments: 203 pages, 39,645 text items, 206,232 glyphs, 1,148 blocks, 27 chapters.

- **Map memory:** 682,526 bytes, 0.68 MB, against the design's estimate of under 1 MB.
- **Layout, median of 40, alternating in one process:** 273.1 ms without a map, 280.4 ms with one, 2.6 percent more.
- **No map against the base build, median of 40, alternating in one process:** 285.9 ms base, 287.4 ms this build. Peak memory footprint 47.2 and 49.3 MB base, 47.7 and 47.8 MB this build, so no change shows. With a map the peak is 48.2 MB.
- **Queries over all 39,317 mapped items:** `position_at` median 3.0 microseconds, maximum 37. `locate` median 83 nanoseconds, maximum 4.8 microseconds.
- **Exactness:** all 199,114 written bytes hold the same text as their chapter's file and `locate` returns their item, with none wrong. 845 spans are inserted and 968 items generated.

The click was proven through gpui's `Window::dispatch_event`, from a probe in a scratch build that is not committed, because mouse events sent with `CGEventPostToPid` never reached the window, focused or not, on this macOS. Keys sent that way do arrive, and caret moves were proven with them. A click from a real mouse is still to be tried.

Temporary until step 5: `editor::engine::face_of`, which the galley's body style uses to find a face by family. `into_parts` and `FontSet` left the `editor` module here.

Where the design was wrong or contradicts itself:

- Section 6 estimates runs as line fragments, as above. The estimate holds anyway, because the map is stored differently.
- Section 6 has `locate` take the layout. Numbering texts in written order and storing their places makes that unnecessary.
- Section 6 lists smart punctuation as `None`, which `pulldown-cmark`'s events give, but its encoding says the parser "records whether the text is verbatim". Comparing the event's text with the source at its offset is what says so, and also handles entities, escapes, and code spans without a flag.

## 2026-11-04: Phase 2 step 5, choices beyond the design

The CLI, the cut `editor` module, the examples, the parity test, and the measurements. [The API record](PHASE_2_API.md) has the signatures as built. These go beyond the design or differ from it.

- **`Layout::font(family, weight, italic) -> Option<&Face>` replaces `editor::engine::face_of`.** The galley needs the four faces of the body family. Any painter that sets its own text in the document's fonts needs the same question answered by the rule inline emphasis uses, so it is public, with a plain `u16` weight and a `bool` slant instead of the config types. `FontSet::nearest` is the version that does not panic for an unknown family.
- **`editor` is cut to what only the desktop's source layer and theme file code need:** `syntax`, `marks`, `theme_value`, `theme_path`, `resolved_config` (the validator's `resolve` and its inputs, and `Config`, `FontStyle`, `Weight`, `of` for the preview), and `engine::{document_theme, theme_file}`. `document_text`, `FontFiles`, `Face`, `Lang`, and `Color` are gone, and the desktop takes a document's text from its own `Input`. Each item carries a one-line reason in `src/editor.rs`. `Scope`, `document_theme`, and `theme_file` are compiled only with `editor` (and `theme_file` in tests), which replaces step 1's `allow(dead_code)` markers.
- **`normalize` and `SourceMap::memory` are documented public items, not `doc(hidden)`.** The CLI needs `normalize` for its paths, and a client that budgets memory needs `memory`. `grep -rn 'doc(hidden)' src` finds only the `editor` module.
- **`Failure` implements `Display` and `Error`,** and `Rendered` and the option types implement `Debug`. A host that uses `?` with `Box<dyn Error>` or `anyhow` had no way to. `Overrides::parse`, `Project::open`, and `DocumentText::read` still return `Vec<Diagnostic>`, which converts with `Failure::from`. A host that wants `?` there writes `.map_err(Failure::from)`, as the examples do.
- **The acceptance test is back in `tests/acceptance.rs` and reads the public `Layout`.** Nothing in it needed a private type, so step 1's move is undone.
- **The parity test compares bytes, not only layout.** `tests/parity.rs` renders every sample, each with its theme, the sample project, and EPUBs through `Renderer::render`, through the stages, and through the CLI binary, with a source map on and off. A book runs where `DRUCK_BOOK` names a project folder, since the book is the owner's and sits outside the repository. Where the variable is unset the test prints a note and runs the rest.
- **Six examples in `examples/`, each a program that asserts what it shows and runs in CI.** They share `examples/common/mod.rs`, which runs the work on a thread with 16 MB of stack: a debug build needs about 1 MB to read a theme, and a Windows main thread has 1 MB. The crate-root doctests use `no_run`, since an example that runs on the same stack rule would need the same wrapper hidden in the docs. `[lib] doctest = false` is gone.
- **CI:** `cargo clippy --no-default-features --lib`, the examples, and `cargo doc --no-deps` with `-D warnings` were added to the `check` job. None has run, since nothing is pushed.
- **A PDF export gives each of its faces a font handle of its own** (`Face::pdf_for_export`). krilla clones a font once per glyph it writes, and with one handle shared by every job on a renderer, four threads exporting the same fonts spent 300 ms on the PDF stage where one thread spent 96 ms. Eight threads spent 1,097 ms. With a handle per export the stage takes 134 ms at four threads and 198 ms at eight, the same as renderers that share nothing, and 96 ms at one. Parsing a face again costs microseconds. The bytes are unchanged on all samples and the book (the guard below).

Measured on the 203-page book (the 27 chapters with their 78 comments, a `print` export, `memoir-print` from the shared folder), on the owner's M1 Pro (8 fast and 2 efficient cores, other work running, load average 2.5 to 4), through the public API in a release build with fat LTO, `/usr/bin/time -l`:

| Case | Time per job | Peak memory footprint (RSS) |
| --- | --- | --- |
| One job in a process, a fresh renderer (the CLI's case), median of 3 | 383 to 419 ms: configure 1 to 3, layout 287 to 320, export 96 to 98 | 47.6 MB (60.5 MB) |
| 20 jobs, a fresh renderer each, one thread, median | 372 ms | 225 MB (254 MB) |
| 20 jobs on one reused renderer, one thread | first 392 ms, later median 374 ms | 238 MB (267 MB) |
| 4 threads, one shared renderer, 12 jobs each | median 699 ms per job, 5.6 jobs/s | 330 to 336 MB (443 to 454 MB) |
| 4 threads, a renderer each | 707 ms, 5.6 jobs/s | 331 MB (449 MB) |
| 1, 2, and 8 threads, one shared renderer | 2.64, 4.26, and 5.33 jobs/s | 145, 213, and 365 MB (174, 270, 596 MB) |

- **Against the limits** of [the release acceptance entry](#2026-11-03-release-acceptance-limits-desktop-stack-desktop-ci), which are 1 s and 128 MB for 100 pages: one job on the 203-page book takes 0.39 s and 48 MB, as in phase 1 (0.38 s, 59 MB), so the public API adds nothing. Reusing the renderer saves nothing measurable on this book (372 against 374 ms); what it keeps, faces and decoded images, costs little to rebuild here, and its worth is in not reading and decoding them again for an editor. A reused renderer does not use more memory than a fresh one in the steady state.
- **A long-lived process uses about five times the memory of one job.** The footprint of repeated jobs rises for about 20 jobs and then holds near 230 to 250 MB, while the bytes the program has allocated are flat at 1.3 MB between jobs (a counting allocator in the benchmark) and `leaks` finds none. That is the macOS allocator keeping freed memory, not a leak, and the same holds for a fresh renderer per job. A service should budget for it, and may prefer an allocator that returns memory. Linux was not measured.
- **No concurrency limit is enforced, and none is needed in the crate,** which has no scheduler. Throughput rises with threads up to about the number of fast cores (2.64 jobs/s at one thread, 4.26 at two, 5.6 at four) and does not rise beyond (5.33 at eight), while the time of one job doubles (0.37 s alone, 0.70 s at four, 1.47 s at eight) and the footprint grows by about 50 MB per job in flight. A client should run about as many jobs at once as the machine has fast cores, and budget about 50 MB of live memory per 200 pages in flight. The rustdoc says this under "Threads". Before the export fix, throughput fell from 4.4 jobs/s at four threads to 3.4 at eight.
- **Cancel to `Cancelled`, 25 cancels at each wait, release build:** median 0.77 ms 20 ms into a layout (maximum 8.8 ms), 0.78 ms at 50 ms (maximum 1.1), 1.50 ms at 100 ms, and 1.05 ms at 200 ms. This is step 3's measurement through the public API.
- **Source map:** 0.68 MB (step 4), unchanged.

Byte guard, release builds of this step against the base `f7d31c7`, with `SOURCE_DATE_EPOCH` set: the 16 sample PDFs (the samples, their themed variants, and the sample project), the 13 sample EPUBs, the book as a `print` PDF, a `draft` PDF, and an EPUB, the sample project's `print` PDF and its EPUB, and the 16 files that the CLI matrix wrote are byte-identical under `cmp` (50 files). The CLI matrix of 87 commands (every sample with `check`, `--print-config`, and `render`; `--set`, `--theme`, `--bleed`, and `--export` errors; bad themes; raw HTML, YAML, image, and wide code errors; a missing file and a folder; a project with a stray chapter, a missing chapter, an open todo, and a print export on an EPUB; `--help`, no arguments, a bad subcommand, and a bad `SOURCE_DATE_EPOCH`) gave the same stdout, stderr, and exit code for each command (57 succeeded, 23 exited 1, 7 exited 2).

Where the design was wrong or left out:

- Section 10 says the parity test "lays out every sample and the corpus". The corpus is outside the repository, so it runs where `DRUCK_BOOK` is set.
- Section 4 expects shared faces to be free to share. They were not, for the reason in the export bullet: sharing an `Arc` that krilla clones per glyph is the one place the shared renderer cost throughput.
- `API_VERSION` stays 1. Everything this step added to the public modules is new, and nothing is published.

## 2026-11-04: Desktop beta slice 1, the document session

The first slice of [the beta](milestones/16-desktop-beta.md). `desktop/src/session/` holds the document without gpui. These choices affect the slices that follow.

- **The editor view holds the session.** Every key needs the active buffer, and helpers that lay out text have a `Window` but no `App`, so an entity would have threaded `cx` through the whole editor. A shared `RefCell` would have traded compile errors for runtime panics. Others read it through `EditorView::session` and change it through `EditorView::update_session`, which keeps the galley in step and emits `SessionChanged`. The panes hold no document: the workspace hands them what they show.
- **The engine still reads the manifest.** A layout calls `Project::open` through `LiveResolver`, which serves chapter files from the session's joined text and everything else from disk. So the manifest's rules, the chapter list's errors, and the real chapter paths in diagnostics and the source map are the engine's own, and unsaved text reaches the pages. The cost is reading and parsing `druck.yaml` and listing `manuscript/` on each layout, about 1 ms.
- **The session reads settings with the engine's parser.** `editor::resolved_config` gained `parse_settings` and `split_front_matter`, because `Project` offers no chapter list and a broken manifest has no `Project` at all. The app has no second YAML reader.
- **`druck.yaml` and front matter change by text splice, checked by the engine.** `session/yaml.rs` finds where a value is written and changes those bytes, as `theme_file/patch.rs` does for JSON. The engine parses the result and must agree with the change before anything is written. A shape the scanner does not read, such as `meta: {a: b}`, is refused with a message rather than rewritten. The manifest is read from disk before each edit, so a change made elsewhere is not lost.
- **A manifest error opens the manifest.** While the book cannot be read, `druck.yaml` is a row in the sidebar and opens in the galley, the problem shows in the sidebar, the details panel, and the preview's strip, and saving it brings the book back. A listed chapter that is missing is a marked row, and saving it creates the file. A file that is not UTF-8 stays untouched.
- **A section of a single file has its own buffer.** That is what keeps undo per section. The file is the sections joined with nothing between them, so the bytes stay the same. A heading that comes or goes re-splits when the caret leaves the section or on a save, not while typing. An unchanged section keeps its buffer, and one that changed starts a new undo history, which is the limit of this choice. A comment directly above a level 1 heading belongs to that heading's section.
- **Rename changes what the title comes from.** A title comes from the first heading, so renaming edits that heading, as an undoable edit that is saved like any other. A project's chapter with no heading has its title from its file, so its file is renamed and the manifest follows. Removing from the book never deletes the file.
- **New section:** in a project, a new file in `manuscript/`, listed after the active chapter. In a single file, text at the end of the active section. Templates use only constructs every theme renders, so no `{.copyright}`: an unknown custom style is an error in a theme without it. A missing `{subtitle}` or `{author}` is also an error, so such placeholders are written out as plain words when the document has no value for them. Starters carry their own theme under `themes/<name>/`.
- **Details write through at once.** A project's go straight to `druck.yaml`. A single file's are an undoable edit to its front matter, and the file is saved with them, including any other unsaved text, since autosave will do that anyway. A `meta` value with several lines shows but is not editable in one field. A cover from outside a project is copied to `assets/`.
- **Revision:** `Session::revision` counts edits to chapters in the book and changes to the book or settings, never an edit to a note, so a note asks for no pages and does not mark the pages stale.
- **A caret in a comment shows at the text after it.** `Layout::locate` answers with the text before a place that sets nothing, which for a chapter's opening comment is the end of the chapter before. `Session::page_position` moves past comments, front matter, and blank lines first.
- **Still limited:** the preview and the inspector keep the theme file found at open, so a `theme:` changed while the app runs takes effect on the next open. Theme core should settle this with its picker. `save` is a plain write until the Persistence slice.

Measured on the 203-page book, as a project of its 27 chapters with their comments, on the owner's M1 Pro with other agents building on it, release build with fat LTO, `DRUCK_GATE=1 DRUCK_GATE_THEME_EDITS=0 DRUCK_CANVAS=split`. The gate types in the body text of the middle chapter, and the comparison is the [spike's run](PROGRESS.md#verification) with the whole book in one buffer, comments stripped, and no source map.

- **Typing, 360 keys:** median 15.1 ms, p95 28.3 ms, max 34.6 ms, against 15.5, 28.8, and 35.1 ms. During a layout: median 24.8 ms against 26.0 ms. With none running: median 13.5 ms against 13.9 ms.
- **Pause to painted pages, including the 150 ms pause:** median 504 ms, max 507 ms with nothing running at the key, against 488 and 492 ms. Median 485 ms, max 487 ms when the key lands just after a layout started, against 618 and 623 ms.
- **Live heap:** 167 MB at refresh 0 and at refresh 25, flat, against 166 MB over 200 refreshes.
- **The sidebar and inspector are cached views.** Without that, every key rendered every row and field beside the galley, and the same run measured a typing median of 17.3 ms. gpui re-renders a view when it is notified, and `window.refresh()` on a change of appearance reaches the cached ones. A cached view must have a size of its own, so its root needs `h_full`.

The click, the drag, and the dialogs were proven through `Window::dispatch_event` from a probe that is not committed, as in [step 4](#2026-11-04-phase-2-step-4-choices-beyond-the-design). A real mouse and the native file dialogs are still to be tried by a person.

## 2026-11-04: Desktop beta, the Shell slice

`desktop/src/shell/` holds the diagnostics and export panels, the sidebar's dots, project search and the find bar, the word count, the string tables, the menus, and settings. Logic without gpui is in plain modules with tests; `Hub` in `shell/mod.rs` connects the views to the document.

- **The string table is one enum per folder.** `strings!` makes an enum with an English and a German text per key, so a key without both does not compile, and it adds a test that no text is empty and both languages name the same `{placeholders}`. `t!(Key::X)` reads the language set at render time, so a language change shows after `window.refresh()` with no restart. Each folder keeps its table in its own `strings.rs` (`shell/text.rs`, `sidebar/strings.rs`, `inspector/strings.rs`, `preview/strings.rs`, `launch_strings.rs`), so slices never edit one file. The language setting is system, English, or German. System reads `defaults read -g AppleLanguages` once and falls back to `LANG`. German follows macOS: Ablage, Bearbeiten, Darstellung, Sichern, Widerrufen.
- **Diagnostics come from the engine's structured diagnostics.** After each layout, `report::build` groups errors, or the latest layout's warnings if there are no errors, by chapter through the source path and line. Placeholders come from the editor's own block and inline scan, which skips code and comments, so no message is parsed. Notes and research are not scanned: only the book can be final. The preview emits `Laid` after any layout, including one that only has errors.
- **A dot means a problem or an open placeholder.** Problem wins over placeholder, and any warning counts, so a book with many loose-line warnings shows many dots. That is what the engine says about the chapter.
- **Word counts use the engine.** The footer needed `Statistics`, which was private. `druck::editor::statistics::count(markdown, lang)` is the smallest addition, in the unstable `editor` module outside `API_VERSION`. Each chapter is counted alone, on a thread, only when its buffer changed, and the book is the sum, so a chapter that closes a directive another opened shows no count.
- **Exports are text splices.** `shell/manifest.rs` reads and changes `exports.<name>.<key>` with the same scanner as the details panel, and the engine must read back the value that was set before anything is written. A last field removed leaves `name: {}`. Deleting an export is not offered.
- **An export runs the public crate on a thread of its own** with a 64 MB stack, and reports three stages. The crate has no progress callback, so a layout of a whole book is one stage. Cancel stops it at the engine's own check points and writes nothing. The file is written to `<name>.part` and renamed. A failure such as an open placeholder in a final export shows in the diagnostics panel at the real lines. A single file writes `<name>.pdf` or `.epub` beside itself, and a project with no named export writes `build/<folder>.pdf`.
- **Search uses `regex`** (MIT or Apache-2.0, already in the lockfile). A replacement needs a non-empty text, applies to the text now in each buffer, and is one undo step per chapter through `Session::edit_chapter`. Notes and research that are not open are read from disk to search and loaded to replace.
- **Menus and keys.** Actions the shell owns are answered by `cx.on_action` handlers that run the work after the action has finished, since the window is busy during it. State lives in the OS app data folder (`~/Library/Application Support/Druck/state.json`, or `DRUCK_DATA_DIR`): window size, up to ten recent documents, appearance, language. A measurement run with `DRUCK_GATE` does not write it.
- **Changes outside `shell/`:** `Session::write_settings`, `Session::load`, `Chapter::is_read`, and the scanner in `session/yaml.rs` are visible to the shell; `EditorView::select`; `PreviewView::zoom_by`, `zoom_to_fit`, and the `Laid` event; `Workspace` can hide the sidebar and inspector and show a canvas by method; `TextInput` has `Edited` and `Cancelled` events, a placeholder, and Enter that keeps focus; the sidebar takes the keys.

## 2026-11-04: Desktop beta slice, editor commands

The floating toolbar, slash menu, block gutter, source view, and chapter notes of [the beta](milestones/16-desktop-beta.md). Command logic is in `desktop/src/editor/commands/` without gpui. Each command is a list of edits and a selection, applied as one `EditKind::Discrete` undo step. No model of the document exists.

- **A mark's state comes from the parsed source.** A mark is on when every visible character of the selection has it, off when none does, and mixed otherwise. Toggling on or mixed goes to on over the whole selection, merging with spans it touches. Toggling on removes the mark from the selected text only, keeping it on the rest of each span, so `**abc def**` with `def` selected becomes `**abc** def`. The Word and Google Docs behaviour, because any other rule for a mixed selection needs a second look at the text to predict.
- **Every mark is checked by parsing the result.** The block must stay one block of its kind, keep every other span, and read as asked, or the command tries whole words and then writes nothing. CommonMark's flanking rules make `**(a)**b` plain text, and checking is simpler than predicting them. `~~~a~~~` at a line start would open a fence, which the same check refuses.
- **Selections grow to well-formed edges.** An edge never lands inside code, a comment, a footnote reference, a citation, a todo, or a `{...}` group, and a span the selection crosses is taken in whole. `_` spans that are cut are rewritten with `*`, since `_` does not pair inside a word.
- **`^x^` and `~x~` mark each word.** They hold no spaces by the engine's rule, and the characters they cannot hold do not count toward a selection being on.
- **A slash entry is offered only where its result is valid.** The engine tests caught a citation in a heading. An image reference needs a real file and a citation a real key, so the entries pick the first image in `assets/`, `images/`, or the chapter's folder and the first key of the bibliography, and are absent if there is none. `toc` is offered once in a book and outside containers, `columns` and `bottom` only outside other directives, and nothing inside a `::: table`. Picking or adding an image with a dialog is the rich blocks slice.
- **On an empty line an entry writes its block with a selected placeholder, which typing replaces.** An empty `# ` would collapse to nothing when the caret left, and several empty forms are errors. With other text on the line, an entry turns the block into its type, or follows it as a new block if it has no such form.
- **Conversions refuse what the engine would reject:** a todo, image, or citation in a heading, a footnote reference in code, whose note would then have no reference, and code lines over 60 characters. A paragraph holding only an image, tables, directives, comments, raw blocks, and footnote definitions have no conversions.
- **Floats are drawn by the galley's frame**, through gpui's `deferred` from prepaint, not as a view in the `overlays` slot. They need the selection's rectangles of that same frame, and a separate view would render a frame behind. The link field is the one float with focus, and because it is drawn inside the pane's dispatch tree, the pane drops its `Editor` key context while it is open.
- **Comments follow the engine, which cuts them before parsing.** `druck::editor` now also exposes `comments`, and `Comments::ranges`. A comment is its own block only with nothing else on its lines. A paragraph holding a comment parses with the comment cut out and its offsets mapped back, so a mark around a comment still pairs. Block scanning is forward only, so `Blocks::apply` starts further back when a comment is still open before the edit. A comment opened inside a line but never closed stays text, where the engine reports it.
- **Limit:** `<!-- c --> # x`, a comment before a heading marker on one line, is a heading in the engine and a paragraph in the source layer. It shows and saves correctly and its pages are right, but its gutter icon and galley style say paragraph.
- **Hidden comments are not edited by accident.** Arrows and Backspace step past a hidden comment rather than into it, Select All in the galley starts after the opening notes, and a caret that reaches one anyway, such as from a diagnostic, reveals it. Only a selection across one can delete it, and that is undoable.
- **The notes panel edits the buffer directly.** It keeps no copy, and its offsets are into the note's text in the buffer, so whitespace and markers around it stay byte for byte and undo is the editor's. Text holding `-->` is refused with a message. It is a new `Tab::Notes`, because the Selection tab belongs to the block under the caret.
- **The source view is the galley with nothing hidden,** per chapter and kept when coming back to a chapter. The same buffer, blocks, caret, and undo apply. Enter writes a plain line ending there, and arrows do not skip blank lines.
- **`Session` gained two reads** for the slash menu, `bibliography_text` and `first_image`. `TITLE_INSET` in `workspace.rs` grew so the title clears the source toggle.
- **The strings are in `editor/strings.rs`, English and German.** The language comes from `LC_ALL` or `LANG`, which a Finder launch does not set. The Shell slice's table should take this over.
- **Source view only, for now:** tables, directive bodies, front matter, link definitions, raw HTML, footnote definitions as blocks, and index marks. They show in the galley as raw text and have no convert entries. The rich blocks slice takes footnotes, citations, tables, and images.

Measured on the 203-page book, as in [slice 1](#2026-11-04-desktop-beta-slice-1-the-document-session), with other agents building on the machine: typing, 360 keys, median 14.4 ms, p95 30.7 ms, max 36.2 ms. During a layout: median 27.6 ms. With none running: median 12.8 ms. Pause to painted pages: median 487 ms. Live heap: 167 MB at refresh 0 and at refresh 25. An idle window in the background, or a galley without focus, drew nothing over 4 s, where the caret used to draw twice a second.

The commands, keys, and the floats' clicks and the gutter's drag were driven through `Window::dispatch_keystroke` and `Window::dispatch_event` from a probe that is not committed. Hover, the feel of a real drag, and IME in the notes field are still to be tried by a person.

## 2026-11-04: Desktop beta, persistence

The Persistence slice of [the beta](milestones/16-desktop-beta.md). `desktop/src/persist/` holds it, without gpui except its one view, and `desktop/src/session/disk.rs` holds what the session knows of its files. Agents and other editors write the chapter files while the app has them open, and losing text is the failure this prevents.

- **Writes are conditional.** A save writes a temporary file beside the target, fsyncs it, and replaces the target only if it still holds what the session last knew. On macOS the replacement is `renamex_np` with `RENAME_SWAP`: the old file comes back and is checked, so a write landing between the check and the rename is put back and reported. A file system without the swap, such as HFS+ or a network share, gets check then rename, with that small window. Permissions are kept, a write goes through a symlink, and a read-only file refuses, since a rename would replace it regardless. `druck.yaml` and themes use the same path.
- **Our own writes are recognized by content.** Each file has a hash of the text it last agreed with. An event only says a path may have changed. If the bytes hash the same, nothing happens and nothing repaints. Reads happen on the main thread and wait for a running write, so a read from before our write can never look like someone reverting it.
- **Timing:** autosave 800 ms after typing stops, at most every 10 s during long typing, on a background thread because `F_FULLFSYNC` takes milliseconds. A recovery copy after 200 ms, at most every 2 s, without fsync. Each key only records the time, so typing costs nothing here. Cmd+S flushes now. Quitting flushes and waits.
- **Watching:** `notify` on the project folder, recursively, filtered on its own thread to `manuscript/`, `notes/`, `research/`, `druck.yaml`, listed chapters, and the theme. `.git` and `build` do not reach the app. Paths wait 100 ms for quiet, at most 1 s. A missing, new, or empty file waits 500 ms, and everything else waits with it, so a delete-and-create, a truncate-and-write, a rename's two halves, and the manifest that names the new file come in together. A folder that moves is reported without its files, so it means a look at everything. A read at start covers what changed before the watch began.
- **Merge:** `similar` diffs lines, Apache-2.0, on which a small diff3 runs. Where both sides changed the same lines it runs again on words from `unicode-segmentation`, so edits in one paragraph merge if they touch different words. Adjacent lines do not overlap, unlike Git. An insertion at the edge of someone's change does not overlap either. Two insertions at one place do, because their order is a decision, so both appending to a chapter asks. A conflict is whole lines.
- **A write from an older read.** A program that reads a file, thinks, and writes the whole file back would take back everything saved since it read, and a merge against the newest version would agree with it. So each file keeps its last 32 versions in memory, and the merge's base is the one closest to what was written. A file exactly like an earlier version is different: nobody edited it, it was put back on purpose by something like `git checkout`, so it counts as a change to the newest. The cost is a surprise in one rare case: a branch made from a version seen minutes ago and switched to right after a commit keeps the newer typing, as unsaved text on that branch. Nothing is lost.
- **Conflict:** while one waits, the buffer keeps its text, the file keeps its text, and saves of that file are held back, also Cmd+S. Typing goes on, and a recovery copy keeps it. A pause in typing tries the merge again, and another change from outside replaces the question. "Keep mine" and "Take theirs" settle only the overlap: what merged stays. The version left out goes to a kept copy first, in `kept/` in the app data folder, and if that copy cannot be written the choice does not happen. "See both" writes both between `<!-- <<<<<<< yours -->` lines, which are comments, so the book still builds and a search finds them. A deleted file with unsaved text asks the same way, and a deleted file never comes back without asking.
- **Undo:** an outside change is one undo step, `EditKind::External`, which never joins another. Undo stays linear, so older typing is behind it. The first undo that reaches one does nothing and says so in the footer. The next one takes it back, keeps a copy of what the file held, and says so. Redo brings it back. I chose this over a barrier because a barrier would make all earlier typing unreachable, and over a plain step because a plain step takes another program's text off the disk at the next autosave with one key. The merge edits are as small as words, so the caret stays on its words. Text inserted right at the caret goes after it. In a single file, a change spreads over the sections it touches, one step each, and a heading that comes or goes re-splits, which starts a new history for that section as [slice 1](#2026-11-04-desktop-beta-slice-1-the-document-session) said.
- **Removed and renamed.** A clean chapter in the book stays a broken row with its text, as the engine's error says, and the same text returns with the file. A clean file not in the book leaves the session. A file gone and one new with the same hash are a rename: the chapter keeps its buffer and undo, and if the manifest still names the old path, the manifest follows. A rename with an edit at the same time is a removal and a new file. If the document's folder is gone, nothing changes in the session, saves fail and say so, nothing recreates the folder, and a slow poll runs only while it is gone.
- **Recovery:** `~/Library/Application Support/Druck/recovery/<name>-<hash>/<hash>.json`, with the path, the base, and the text, cleared after a successful save. At open a copy becomes one undo step and then merges with whatever the file holds by now, with the same rules as any change. `DRUCK_DATA_DIR` replaces the folder for tests. Kept copies are not cleaned up. That should be decided before 1.0.
- **Input methods:** outside changes wait while text is being composed.

Measured on the owner's M1 Pro with other agents building on it, release build with fat LTO, scratch copies of the 27-chapter book with `DRUCK_CANVAS=split`.

- **Typing, 360 keys, with autosaves and recovery copies running during it,** two runs each of this build against the build before it, `DRUCK_GATE=1 DRUCK_GATE_THEME_EDITS=0`: median 14.4 and 14.1 ms against 14.2 and 14.3 ms, p95 29.8 and 26.5 ms against 28.7 and 29.6 ms, max 36.7 and 33.9 ms against 35.3 and 35.4 ms. No difference beyond the runs' own spread.
- **Pause to painted pages, including the 150 ms pause:** median 487 and 488 ms against 488 and 475 ms.
- **Live heap:** 167 and 168 MB, flat over 25 refreshes, against 167 MB.

The soaks ran with a Python writer that picks a line, adds a unique word, and replaces the chapter: 70% by atomic rename, 15% by delete and create, 15% in place. It ran against keys sent by PID to the same chapter, and each run ended with Cmd+Q and an exact comparison with the expected text, which every run passed: 300 s with reads up to 2 s old (158 writes, 431 typed words), 300 s in the same paragraph as the typing with reads up to 1 s old (217 writes, 431 words), 240 s with fresh reads (278 writes, 354 words). At idle for 60 s the watcher processed no batch and the app used 0.21 s of CPU, with the `notify` thread parked in its run loop. 200 writes to `.git` and `build` reached nothing. `git stash`, `stash pop`, `checkout --`, and branch switches ran under the app, and a branch switch rewriting all chapters took 22 ms on the main thread in one batch. Also proven live: SIGKILL with a recovery copy and a file changed meanwhile, a read-only file, a moved folder, and a full disk on HFS+ with the fallback.

## 2026-11-04: Desktop beta slice 4, theme core

The fourth slice of [the beta](milestones/16-desktop-beta.md). The Styles tab holds four panels (styles, tokens, page, source), the Fonts tab one, and the title bar the theme picker. These choices affect the slices that follow.

- **One model, one editor.** Every panel changes the theme through `ThemeModel`, which wraps `theme_file::editor::Editor`, a plain struct with no gpui. A change is a text splice, so a partial theme stays partial and the author's formatting stays. Renaming a token renames the key in place and every string value that names it, in objects and arrays alike. The source view replaces only the bytes of one section.
- **A change is accepted when the engine renders it.** `resolve` alone was not enough. The cross-property test (`every_change_the_panel_can_make_is_refused_or_gives_a_theme_the_engine_renders`) found three things it accepted: a font family that is neither in `fonts` nor installed, a weight the family has no face for (`body.weight: 600`), and a list style in italic or in the mono font, whose nested bullet has no glyph. Each stops the CLI. So a change now has to pass `resolve`, the engine's installed font lookup (a one-line re-export in `src/editor.rs`), and a typeset of a short probe document from a trial file beside the theme, which is removed again. A theme that already fails to typeset can be mended in steps. The probe is a typeset on the UI thread for each committed change, and the numbers below include it.
- **A common theme in a project asks before the first edit.** The choice is a copy in the project, recommended, or the shared file. A copy goes through `library::fork`, which takes the fonts and images along, and a project theme wins over a common one of the same name, so documents that name the theme use the copy with no change to `theme:`. The question names the open document only. The app has no index of other documents, so it says that others are not known. The choice to edit the shared file lasts until another theme opens.
- **The inspector and the pages follow the document's `theme:`.** The preview resolves the theme at each layout, as the CLI does, and the workspace reopens the inspector's file whenever the document names another. A theme named on the command line overrides that until the author picks one. This replaces the limit of [slice 1](#2026-11-04-desktop-beta-slice-1-the-document-session).
- **Style to block needs no engine change.** The source map gives each block's kind and place, and the style follows from the kind, the blocks around it, and a `{.name}` at its end (`theme_file/blocks.rs`). It cannot see a `{.name}` on the line before a list, a table row's class, or text outside every block. The title block, running heads, and watermark go to the first page. A style with no block in the document is noted for the Styles panel, which offers the specimen. The specimen has no bibliography, since that needs a `.bib` file beside the text.
- **The specimen** is `desktop/assets/specimen.md`, typeset by a second `Typesetter` in the same theme. Pages show it on request. While it shows, document edits only store their text, and a theme edit typesets it again. Clicks on it do nothing, since its offsets are in no chapter.
- **The text field reuses `source::Buffer`,** with its undo and UTF-16 maps, and `Selection`. The galley's element is bound to a session and `BlockKind`, so the field has its own small element. It takes IME composition and mouse selection (drag, word, line). What remains: a field inside a scrolling panel does not scroll to keep its caret in view, no text drag and drop, and no accessibility tree.
- **Strings** are `Text` values with English and German in `inspector/strings.rs`, which reads `DRUCK_LANG`, `LC_ALL`, or `LANG`. The Shell slice's table takes them over when the two meet.
- **Panels and the 30 sections.** Designed: `styles` and `custom-styles` (every property of the schema), `tokens`, `fonts` (the families in use and adding a file, not editing faces by hand), and `page` (size, margins, mirror, column gap, text width; not `wide`, the column change spacing, or the header and footer offsets). The other 25 are the source view only: `$schema`, `version`, `document`, `images`, `inline`, `lists`, `tables`, `callouts`, `footnotes`, `margin-notes`, `placement`, `captions`, `bibliography`, `toc`, `index`, `matter`, `chapters`, `scene-break`, `typesetting`, `title-block`, `title-page`, `pages`, `watermark`, `highlight`, and `labels`. The list is `SECTIONS` in `inspector/source_view.rs`, and the source view shows each one's tag.
- **Measured** on the 203-page book as a project of its 27 chapters, `memoir-print` as a copy outside the common folder, release build with fat LTO, on the owner's M1 Pro with other agents on it, `DRUCK_GATE=1 DRUCK_CANVAS=split`. Theme edit to painted pages, 12 samples: median 358 ms, p95 390 ms, max 397 ms, against 338 ms before the slice; the galley changes at the same time. The edit now includes the probe typeset and the jump to a block in the style. Typing, 360 keys: median 14.8 ms, p95 28.5 ms, against 15.1 and 28.3 ms. Pause to painted pages: median 507 ms with nothing running and 503 ms just after a layout started, against 504 and 485 ms. Live heap 168 MB at refresh 0, 25, and 50.
- **Changes outside the slice's folders,** each the smallest that works: `Kind::base_dir`, `Session::theme_setting`, and `Field::Theme` in `session/` (with two match arms in `inspector/details.rs`); `Job`, `set_theme`, `set_specimen`, `refresh`, `fonts`, `find_style`, and the fonts a layout reports in `preview/`; the wiring in `workspace.rs`; one line in `app.rs`; `installed_fonts` in the root crate's unstable `editor` module.

## 2026-11-04: Desktop beta, agent panel

The sixth slice of [the beta](milestones/16-desktop-beta.md). `desktop/src/agent/` holds the panel. Its logic has no gpui and is tested against the recorded stream in `desktop/tests/fixtures/agent-turns.jsonl`.

- **A tab of the right rail, not a column.** The mockups' right pane is 296 px beside a 40 px rail. A fourth column of about 360 px would come out of a canvas that is 1088 px at the default window and already split into galley and pages. The conversation scrolls and the approval, the chips, and the prompt stay at the foot, so a waiting approval is always on screen. While one waits and another tab is open, the rail button carries a dot (`InspectorView::set_attention`).
- **Nothing starts until it is used.** The CLI is looked for when the tab is first drawn, and the child process starts with the first prompt. It ends on a new conversation, a model change, closing the window, quitting, or opening another document. The real CLI also exits when its stdin closes, which was checked without sending it a prompt, so an app that is killed leaves no child either.
- **The CLI runs as it does in a terminal.** No `--tools`, no `--setting-sources`, no `--strict-mcp-config`, a test checks the argument list. So the author's own settings decide what is asked. With `defaultMode: auto`, as in the owner's settings, edits are allowed without a question, and the panel still shows each one and its diff. The approval cards were exercised in a scratch project whose `.claude/settings.local.json` sets `defaultMode: default`.
- **The login is the CLI's.** The panel runs `claude auth status` and keeps only the method and plan from its JSON, never an account name, organization, or token, and says in one line what it reports. `DRUCK_CLAUDE` names the program for an install the search does not find, and was used to show the setup and untested-version states. The setup text names an API key as the documented way. When the reported method is a subscription login, the panel adds that Anthropic's permission for it in an app like Druck is not confirmed. **Before any public build includes the panel, the owner must have Anthropic's written answer on spawning the user's own CLI with a subscription login.** This is also at the top of `agent/mod.rs`.
- **Cost is a difference.** `total_cost_usd` counts everything one CLI process has spent: 0.00127 and 0.00161 in the same process, then 0.00009 after a restart. A turn's cost is the difference, and a new process starts again from zero. A turn cut short by Stop reports no new cost and shows none.
- **Approvals.** The first waiting one shows with the file and a diff: an Edit as the whole lines it touches with two lines of context, the changed words emphasized and long lines cut around them, and a Write as a line diff against the file on disk. Choices are allow once, deny with a reason the model sees, and, only when the CLI's `permission_suggestions` holds one with destination `session`, a button for it, passed back unchanged as `updatedPermissions`. A suggestion that would write to a settings file is not offered, since it outlives the panel. The scope sentence says it lasts until Druck closes or a new conversation begins. Stop denies every waiting approval before it interrupts, since the CLI waits on them. Nothing times out. The chapter having unsaved text is said on the card, since the agent edits the saved file.
- **Hunks are found by their lines, never held as positions.** `agent/changes.rs`: a `structuredPatch` hunk is found again in the buffer by its lines, nearest to where it was last seen, so text typed above it, a reload, or a second edit do not lose it. A hunk whose lines the author rewrote reads "text changed since" and offers no revert, so a revert can never remove the author's words. A revert is one `edit_chapter` transaction, so one undo step, and undo brings the agent's text back. The galley tints the passages of the newest turn with changes (`EditorView::set_marks`), hides them while the author types, and brings them back 150 ms after. Revert and Show are in the panel's turn summary, not on the marks, because the galley element has no clickable overlays. A deleted passage has no mark.
- **The seam to Persistence is `agent/carry.rs`, and it is temporary.** Until Persistence reloads changed files into buffers and merges them, each patch an approved Edit or Write reports is applied to the open buffer of that file, hunk by hunk as an ordinary edit. This is a three-way merge in effect: the agent's patch is the change from the disk text before to the disk text after, and the buffer is the author's side. Edits elsewhere in the chapter stay, and a hunk on lines the author also edited is left alone and said in the conversation, tested against a real session. Persistence replaces `carry` and must keep that property. The diff layer needs from it only the buffer text through `Session::buffer` and a `SessionChanged` or `TextChanged` after any change. A change that arrives with no patch, such as one made through Bash, is not seen until Persistence reloads it, and a Write or Edit without a patch says so in the conversation. A file the session has not open shows its hunks as "not open here" with no revert.
- **One session per project, in the app's data folder.** `agent-sessions.json` maps a project's canonical path to its session id and model, in `~/Library/Application Support/Druck` on macOS, `$XDG_DATA_HOME/druck` or `~/.local/share/druck` on Linux, `%APPDATA%\Druck` on Windows, or `DRUCK_DATA_DIR`. It is written beside itself and renamed, and never goes in the project. The next prompt after a restart starts the CLI with `--resume`, which was checked: the model named the first request of the earlier conversation. The earlier rows are kept by the CLI and are not shown again. A resume the CLI rejects clears the stored id. New conversation clears it too.
- **Context is a snapshot the author can see.** The chapter path, the selection, and the diagnostics show as chips, taken when the prompt box gets focus or the chapter changes, and what is sent is what the chips show. A removed chip stays removed until the prompt is sent. Selections are cut at 6,000 characters and diagnostics at 20, and the sent prompt row says what went with it.
- **The prompt box takes input methods.** It is its own small editor on the source buffer's `Buffer`, with the editor's input handler, so dead keys and composition type as in the galley. A dispatched sequence with an umlaut, a new line, arrows, and backspace gave the expected text. Mouse selection by dragging and double clicks are not built. Replies cannot be selected or copied yet.
- **When the CLI changes.** `cli.rs` marks 2.1.317 and later patch releases of 2.1 as tested. Anything else shows an "untested" line and still works. Events the panel does not know are kept raw and change nothing, and do not redraw the panel. A line that is not JSON becomes an error row. A changed flag, most likely `--permission-prompt-tool stdio`, which has no entry of its own in `--help`, ends the process, and its stderr shows as the error. Without `permission_suggestions` there is no session button, and without `structuredPatch` there are no diffs and the conversation says so.
- **First text depends on the model.** The CLI said it had started 0.4 s after a prompt and a three word reply began at 1.8 s. A 350 word German reply began after 10.5, 15.7, and 17.1 s in three runs, because the model thinks first, and the panel shows "Working..." until text arrives.
- **Strings.** `agent/strings.rs` holds English and German side by side, chosen by `DRUCK_LANG`, else `LC_ALL`, `LC_MESSAGES`, or `LANG`. The Shell slice's table should replace `strings::language`.

Measured on the owner's M1 Pro with other agents building on it (load average 5 to 28), release build with fat LTO, the 27-chapter book as a project, `DRUCK_CANVAS=split`, keys every 90 ms into the middle chapter through the input handler, the Agent tab open, and `--model haiku`:

- **Typing with the panel open and no turn:** median 12.5 to 12.7 ms, p95 19.6 to 20.4 ms, over 40 keys in each of three runs.
- **Typing during a turn:** median 13.3 and 13.5 ms over 264 and 306 keys in two runs that did not tell thinking from streaming. In the third run, 15.0 ms median over 139 keys while the model thought and 13.7 ms over 47 keys while text streamed in, p95 19.8 and 19.9 ms. The spread between runs is the load of the machine. All stay under 16 ms.
- **Redraws:** the panel redraws at most every 16 ms while a turn runs, only when an event changed what it draws, and the CLI's output is polled from a timer, 250 ms while idle.

The live runs used 14 prompts to `haiku` in scratch copies of `buch-1-ttp404` and never touched the book. They showed a denied edit with a reason (the file and the buffer unchanged, the reason in the model's answer), an allowed edit with its hunk, a revert of one of two hunks and the file after Cmd+S, "allow all edits this session" followed by a second edit that did not ask, Stop, and a resume after a restart. Clicks were sent with `Window::dispatch_event` from a probe that is not committed. A real mouse is still to be tried by a person.

## 2026-11-04: Desktop beta, integrating slices 2 to 6

The five slices are one branch, `t3/integrate-desktop-beta-slices`, merged in the order Shell, Editor commands, Persistence, Theme core, Agent. This entry replaces what the slice entries call temporary or leave for the integrator.

- **Strings.** The editor, persistence, inspector, agent, and session tables are per-folder tables made with `crate::strings!` and read with `crate::t!`, so the language setting reaches them and a Finder launch no longer falls back to English. `inspector::strings::Key` keeps `IntoElement` and `Into<SharedString>`, since the theme panels pass keys around as data. The slash menu takes a `Language`, so its tests stay deterministic and its search still matches both languages and the group name. Section templates, starters, and `Group::label` are translated. The templates' Markdown stays as written.
- **German wording.** "Theme" is one word in both languages, since "Thema" reads as the book's subject in an app for writing books. Common themes are "gemeinsam" throughout. Paper sizes are "Letter" and "Legal", because "US-Letter" touched its neighbour.
- **The agent sees files through Persistence.** `agent/carry.rs` is gone. Persistence alone takes what is on disk into buffers, and the panel builds a turn's hunks from each `FileChange` it hears while a turn runs, `base` to `theirs`, grouped by chapter id so a rename keeps them together. A write that follows the last one extends it, and one that does not is its own step, so the author's autosaved text between two agent edits is never shown as the agent's. Hunks come from a new `merge::line_changes`, the line level under `edits_between`, since a hunk is whole lines and rebuilding lines from word edits would only undo that work. So Bash edits have hunks. The CLI's own patch is kept only for a file the session does not have, shown as "not open here".
- **A revert is a merge,** replaced by [a reject that replaces the hunk's lines](#2026-11-05-writing-partner). `merge::three_way` with `theirs` as base, the buffer as mine, and `theirs` minus that hunk as the other side, applied through `edits_between` as one undo step. It works on bytes of the two versions, so line ends and a missing final line break survive. A hunk whose lines the author edited reads as changed since and offers nothing, and a conflict on arrival is reported in the conversation while Persistence's footer asks.
- **The end of a turn.** `scan_now` looks at once, and changes count for 500 ms more. `scan_now` now waits while a save or composition runs, since a read during our own write would look like an outside change and credit the author's typing to the agent. **Limit:** any outside change during a turn counts as the agent's, as nothing tells two programs apart.
- **`FileChange` lost `before`, `after`, and `at`.** Nothing reads them.
- **The theme on disk.** The watcher follows the theme the inspector has open, through `ThemeSwitched`, not the one named at mount, so a picked or forked theme and one outside the project are watched. `ThemeModel::reload` does what `StylesPanel::reload_theme` did, and an empty `property` means everything. A `theme:` changed in `druck.yaml` takes effect while running.
- **Details no longer save.** A single file's Details are an undoable edit in the buffer, and autosave writes them with whatever else is unsaved. A fork writes only the theme.
- **Dev hooks have one gate.** `gate::var` reads environment variables only in a debug build or with the new `measure` feature. Everything that changes what the app does goes through it: `DRUCK_TAB`, `DRUCK_SUBTAB`, `DRUCK_STYLE`, `DRUCK_FONT`, `DRUCK_SPECIMEN`, `DRUCK_PICKER`, `DRUCK_PROBE_EDIT`, `DRUCK_CANVAS`, `DRUCK_GATE` and its options, `DRUCK_TRACE`, and `DRUCK_PREVIEW_*`. `DRUCK_LANG` is gone. `DRUCK_CLAUDE` and `DRUCK_DATA_DIR` stay in every build, as documented overrides. Measuring now needs `cargo build --release -p druck-desktop --features measure`.
- **The CLI's search path.** An app started from the Finder has only the system's `PATH`, so the CLI could be found yet not find `node`, `git`, or MCP servers. The panel asks the login shell for its `PATH` once, off the main thread, with a 3 second limit, adds the usual folders, and gives the result to the CLI and everything it starts.
- **The root clippy failure.** `Statistics::count` was `pub`, and re-exporting `Statistics` from the `editor` module made `Document` and `Inline` publicly reachable, so their `pub(crate)` field types `Sources` and `Spans` counted as leaks. `count` is now `pub(crate)`, which is the cause, rather than widening the two types.
- **Breaks only the combination showed.** The word count and the saving state each took half the footer, which cut a conflict's sentence off with room to spare, so the word count takes only what it needs. The specimen's warnings replaced the document's in the diagnostics list and the sidebar's dots, because Theme core's specimen and the Shell's `Laid` never met, so `Laid` now skips the specimen. The title bar's full path ran into three slices' toolbar, so it shows the name, and the window title keeps the path.

Measured on the owner's M1 Pro, release build with fat LTO and `--features measure`, on a scratch copy of the 203-page book as a project, with the sidebar, inspector on the Agent tab, watcher, autosave, and split canvas, no other agent running, load average 3 to 4, two runs of `DRUCK_GATE=1 DRUCK_CANVAS=split DRUCK_TAB=agent DRUCK_GATE_REFRESHES=200`:

| | This branch, two runs | Slices alone |
| --- | --- | --- |
| Typing, 360 keys, median | 15.4, 15.0 ms | 14.1 to 15.1 ms |
| Typing, p95 and max | 29.1 and 35.7, 28.9 and 34.8 ms | 26.5 to 30.7, 33.9 to 36.7 ms |
| Typing during a layout, 62 keys, median | 25.7, 25.7 ms | 27.6 ms |
| Typing with no layout running, 298 keys, median | 13.6, 13.6 ms | 12.8 ms |
| Pause to painted pages, median | 504, 504 ms | 487 to 507 ms |
| Key during a layout to painted pages, median | 485, 485 ms | 475 to 503 ms |
| Theme edit to painted pages, median | 386, 371 ms | 358 ms |
| Live heap over 200 refreshes | 180 MB at every 25th | 167 to 168 MB over 25 to 50 |

Typing and pages are inside the range the slices measured alone, under the 16 ms and 1 s targets. The theme edit is the one regression, 13 to 28 ms. A committed edit takes a median of 64 ms on the main thread inside `Editor::apply`, which holds the probe typeset, and Theme core's plain write and rename became Persistence's `atomic::write` with `F_FULLFSYNC`. Timed in a scratch build, the Shell's report adds 1.1 to 1.9 ms after a layout. The fsync was not timed apart from the probe, so that attribution is by elimination. Moving the save off the main thread would take it back. The heap is 12 MB above the slices' numbers and flat.

The combined soak, on the same build: the persistence soak's writer rewrote `25-k07.md`, 149 times by atomic rename, 30 by delete and create, 23 in place, from reads up to 2 s old, while keys sent by PID typed 319 words into a line of the same chapter, with autosave and the pages running and the Agent tab open. After 300 s and Cmd+Q the file was exactly the original plus the writer's words on their lines and the typed text in one piece. An earlier run, two commits before, had 202 writes, 315 words, and the same result. No text was lost.

The live agent run used two turns of `haiku`, in a scratch copy with `.claude/settings.local.json` setting `defaultMode: default`. An Edit waited at its approval card while text was typed into another paragraph. Both ended up in the buffer and on disk, the hunk was marked, and its revert kept the typed text. A `sed -i` through Bash produced a hunk with no patch from the CLI, which the model itself could not confirm from the command's silent output.

## 2026-11-04: Desktop beta, rich blocks

Slice 7 of [the beta](milestones/16-desktop-beta.md): footnotes, citations and cross-references, tables, images, and the Selection and Images tabs. The logic is in `desktop/src/editor/rich/` without gpui, one module per kind of block, and the views are in `rich/view/`. Every editor edits the Markdown through the buffer as `EditKind::Discrete`, one undo step, and nothing is written from a model: a table's grid is read again from the text after each edit.

- **Chip text comes from the engine.** `druck::editor::rich::labels` parses a configured document and gives the text of every citation and label, so chips say what the pages say: numbering, `Knuth 1984a`, and the theme's words. It reads off the main thread with its own `Typesetter`, and only when a hash of what chips depend on changes: citations, references, headings, and captions. Plain typing runs no engine at all. A key costs a table lookup when its block is shaped. Until the engine has read a new citation, its chip shows who and when from the bibliography.
- **A citation of an unknown key leaves the others alone.** The seam adds citations one by one and skips what the bibliography lacks, where `citations::resolve` fails whole, so one wrong key does not empty every chip. Unknown keys and labels are found from the text and show as error chips. The engine's own diagnostic stays in the list, and the picker says the same.
- **A page reference's chip reads "page of Table 1".** The page number exists only after layout, and `Layout` keeps no map from a label to its anchor. It could, for a later change.
- **Footnote labels follow the chapter, then the book, across chapters.** The chapter's most used stem with the next number free in the book, else the book's stem with the file's word in it (`k01-` in a book means `k15-` in `46-k15.md`), else `note-`. Labels are compared without case, as the engine does.
- **A definition goes where the text keeps them.** Definitions with only comments and blank lines between them are kept together, and a new one goes in reference order among them. The book's chapters do this, some with a closing line after the notes. Otherwise it goes below the block. A chapter with none follows what most chapters of the book do.
- **A referenced definition has no block of its own in the galley.** The popover edits it, including in another chapter. Its field holds a plain copy, with the indent off, and closing it writes the change, so one popover is one undo step in the chapter that keeps the definition. Lines that did not change keep their bytes, however they were indented. A new note closed with its placeholder untouched takes itself back as one undo.
- **Deleting a reference is found by reading the book, not by watching keys.** After a pause, a definition that lost its last reference gets the offer. That covers Backspace, a selection, a raw edit, and an agent. It takes no focus, because this app does not interrupt typing, and leaves with the next edit. The definition then shows as a raw block, so it is never orphaned silently. Removing it joins the deletion's undo step through `Buffer::amend` when nothing happened in between.
- **Validity is the parser's call on the whole book.** `rich::check` parses the joined text before and after an edit, and refuses what brings a new problem, in the engine's words. So a book with problems can still be edited, and an edit never adds one. It costs about two parses of the book at each commit, not at each key. Labels with `ß` are refused this way: label names are ASCII by the engine's rule.
- **A table is one float with one moving field.** The cells that are not active are plain text, so a large table holds one input. A pipe table's cell keeps the width it was written with as far as its text allows, a cell lined up at the right stays so, and no other row changes. Rows and columns are edits per line. A list table's cell keeps its own lines and indent, and `{span=n}` merges only an empty neighbour, since merging text would lose it silently.
- **A list table's `align` needs a word for every column,** so choosing an alignment for one column writes `left` for the others if the directive has none. "As the theme sets it" cannot be said per column there. A pipe table can.
- **Figures and tables show whole, and a caret never stands in them.** They are drawn from the block's layout, with the source lines collapsed under them, so the block keeps its height and the caret maps stay true. A caret brought inside, by a diagnostic or the find bar, shows the source. A file's size comes from its header when shaping, and the full decode the engine would do happens when a file is taken in and on another thread for the Images tab.
- **Taking in an image.** A copy goes to `assets/` in a project and beside the document for a single file, reserved by creating the file so a name is never overwritten. The same bytes under the wanted name are the same image. A file already in the document stays where it is. The original is never moved.
- **Alignment is a named style.** No per-block attributes exist, so choosing an alignment picks or makes `body-center`, based on the block's current style, in the theme file through `ThemeModel`. A common theme asks its question first, and the block gets the style when the answer is in.
- **`Session` lost `first_image` and the slash menu `Action::Footnote`, `Citation`, and `Image`.** Those entries now open the editors. A citation is offered when the bibliography has a work, a cross-reference when the book has a label, and an image always.
- **Changes outside this slice's folders.** `source/inline.rs` gained `SpanKind::CrossReference`, read by the engine's rule. `source/buffer.rs` gained `Buffer::amend`. `inspector/input.rs` gained `selection`, `apply`, and `in_prose`. `preview/job.rs` gained `Typesetter::labels`. `src/editor.rs` gained the `rich` module, for which `Configured::job` is `pub(crate)`, `citations::reading_order` and `pipeline::load_bibliography` are `pub(crate)`, and `Structure::label_texts` and `bibliography::read_works` exist under the `editor` feature. CLI output is untouched: none of this runs without that feature.
- **Source view only:** directive bodies other than `::: table`, front matter, link definitions, raw HTML, code, index marks, comments outside chapter notes, a citation that spans lines (it stays quiet raw text), and any table the grid cannot read, which says why and offers the source view. A pipe table in a list item or a quotation stays raw, as its block is the list or the quotation. Row and cell styles of a list table are kept and shown in the cell's text, with no picker. The cover and details fields stay in Details.

Measured on the owner's M1 Pro, release build with `--features measure`, on a scratch copy of the 203-page book as a project, with the sidebar, inspector, watcher, autosave, and split canvas, and 12 paragraphs of three citation chips each added around the typing place, marks in the book's other chapters, and another agent's builds on the machine. `DRUCK_GATE=1 DRUCK_CANVAS=split DRUCK_GATE_THEME_EDITS=0 DRUCK_GATE_REFRESHES=25`, two runs on each tab:

| | Selection tab | Agent tab | Before this slice |
| --- | --- | --- | --- |
| Typing, 360 keys, median | 15.5, 15.5 ms | 15.7, 15.8 ms | 15.4, 15.0 ms |
| Typing, p95 and max | 28.3 and 34.2, 30.0 and 35.0 ms | 29.1 and 36.1, 29.5 and 34.9 ms | 29.1 and 35.7, 28.9 and 34.8 ms |
| Typing during a layout, median | 25.4, 26.9 ms | 26.0, 26.4 ms | 25.7, 25.7 ms |
| Typing with no layout running, median | 14.1, 14.3 ms | 14.2, 14.2 ms | 13.6, 13.6 ms |
| Pause to painted pages, median | 491, 504 ms | 504, 504 ms | 504, 504 ms |
| Live heap over 25 refreshes | 170 MB, flat | 170 MB, flat | 180 MB over 200 |

Typing stays under the 16 ms target, about 0.5 ms above the integration's numbers, and the Selection tab costs nothing measurable. The gate types into the galley. A floating card takes no keys from the galley while it is on screen, and typing inside a popover's own field was not timed.

The commands, keys, and clicks were driven through `Window::dispatch_keystroke` and `Window::dispatch_event` from a probe that is not committed, on a scratch copy of the book and a small project with tables and images. The native file dialog, a real drop from the Finder, hover, and input methods in the popover and grid fields are for a person.

## 2026-11-04: Desktop beta gate, saving and packaging

- **Theme saves run on a thread.** `Editor::stage` makes a change and checks it at once by resolving the theme as a render would, so a refused value is reported where it is typed. The probe typeset and the `atomic::write` run on `theme_file::writer::Writer`, one thread that takes commits in the order they were made, so two quick edits cannot land the other way round. The writer keeps the text each file held after its own writes, so a commit that was refused leaves the next one judged against what is really on disk. `ThemeChanged` is sent when the write has landed, because the pages read the theme from the file. A commit the probe refuses is never written; once nothing more is queued, the panels go back to the text on disk and the reason shows as a rejection in the Styles detail, the JSON view, and `problem_at`. Quitting waits for the writer (`ThemeModel::flush`, from `PersistView::flush_blocking`). A font file copied in by `AddFont` stays in the theme's `fonts/` folder if its commit is refused.
- **Measured** on the 203-page book as a project, `memoir-print` as a loose copy, release build with `measure`, `DRUCK_GATE=1 DRUCK_CANVAS=split`, 12 edits, same machine and session, old and new binaries both with the new "window thread busy" line. The window thread was busy with a committed edit for a median of 65.2 ms before and 34.5 ms after. Theme edit to painted pages: 390.7 ms before, 389.1 ms after, and to the painted galley 388.0 and 389.3 ms. The end to end time did not move, because the layout waits for the write and the write now waits for the probe, as before. Typing, 360 keys: 16.0 and 15.7 ms median. The write alone, file and folder `F_FULLFSYNC` on this disk, 30 runs: median 9.0 ms, p95 12.0 ms, max 23.6 ms, against 0.2 ms with no sync. So the fsync was 9 of the 64 ms, not the larger part the integration note guessed: the probe is about 22 ms and the rest of the old 65 ms is the resolve check, which still runs on the window thread (about 34 ms with the panel's own work, not profiled further). The gate now waits for the pages to be requested before it times them, since the request follows the write.
- **Closing and opening ask only when text would be lost.** `FileSync::write_all` writes every file that changed, takes in a file someone wrote in the meantime (a merge or a conflict) and writes the merge, keeps recovery copies of what is left, and returns `Closing`: `Saved`, `Held`, or `Failed`. Opening or closing a document goes through `persist::settle`, which waits for a running autosave, then runs that. `Saved` and `Held` go on without a question. `Held` is text in a conflict the footer holds: the file keeps their version, a recovery copy keeps mine, and the next open merges it, as persistence specifies. Only `Failed` asks, with the file and the reason: try again, discard, or cancel. Cmd+Q and the window's close button end in `flush_blocking`, which is now the same `write_all`, so they never asked and still do not. This replaced `Session::save` (now test only), `EditorView::save_all`, and `Workspace::save`.
- **The app bundle.** `scripts/package-desktop.py` writes `Druck.app` and a DMG with `hdiutil`. Identifier `com.coderscantina.druck`, a guess from the repository's owner to confirm. The version is the root `Cargo.toml`'s, and `druck::editor::VERSION` gives the About box and `--version` the same string the CLI prints. The minimum macOS is read from the binary. Markdown and folders are document types with rank `Alternate`, so Druck is offered and does not take over `.md`. Macs hand an opened document to the app as `application:openURLs:`, which gpui exposes as `Application::on_open_urls`; `open_urls.rs` turns the file URLs into paths and the window opens the first. The icon is a placeholder the script draws.
- **Signing is scripted and the repository holds no secret.** `DRUCK_SIGN_IDENTITY` and `DRUCK_NOTARY_PROFILE` come from the environment. Without them the bundle is signed ad hoc and the script says so. The hardened runtime is on and the entitlements file is empty: nothing needed an exception. With both, the app is notarized and stapled, then the DMG. CI builds the app ad hoc and keeps it as a workflow artifact the publish job does not take.
- **Licenses.** `notices.py` finds every text the crates ship. The 41 desktop crates with no text ship none in their source, so the script now adds the standard MIT and Apache-2.0 text for the ones that declare those, in a table with the authors their manifests name. It adds no Zlib text, which asks for no notice with a binary. Which license to follow where a crate offers a choice, and whether manifest authors are enough for the MIT copyright line, is the owner's decision.

## 2026-11-04: Desktop beta, finishing the gate

**Print-composer lines.** `druck::editor::composer::lines(configured, measure)` runs the page layout's flow with the prose set to `measure` and stops before pagination, so there is no second line breaker. It returns each line of running text with its place in the written text, whether the engine added a hyphen at its end, whether it reaches the measure, and its left edge. `tests/composer.rs` compares it with real pages at the page's measure: every line start agrees on all samples and on the 203-page book, 3,680 of 3,680 lines, in 0.5 s for all of them in a release test.

- **The galley keeps the toolkit's glyphs.** Painting the engine's glyphs would be a second font path beside gpui's, for no gain in where lines break. The galley shapes each line once without wrapping, then writes its own wrap boundaries and glyph positions onto gpui's `WrappedLine`: the forced row breaks, the first line's indent, and for lines that reach the measure, the extra per space. The positions stay rising across rows, so the caret, the selection, and `closest_index_for_x` work unchanged. An added hyphen is painted after its row and is not in the text. Fonts are the same family as the page, since the layout registers them.
- **The column narrows to the page's measure** when the toggle is on and the pane has room, at the galley's fixed scale of 1.4. The brief said "at the column width", and a column of 648 px would have shown lines nobody will print. Narrower panes compose at what they have.
- **When it runs:** 600 ms after the last key, on a thread of its own, with a `Cancel` that the next key trips, and again after a resize or a theme change. On a key only a timer is replaced. It costs about one more flow per pause, far less than the pages' layout. Results are kept by each block's text, so a block that did not change keeps its lines through edits elsewhere. A block typed in keeps toolkit lines until the caret leaves it.
- **Measured on the book, in the app, all chapters:** 3,679 rows in the galley, 3,670 start where the page's lines do. Six are the copyright page's paragraphs in a smaller custom style, which fall back to toolkit lines because a row in the galley's size would run 10 percent past the column. Three are hyphenation cuts inside a toolkit ligature (`geschaf|fenen`): the row can only start after the glyph, so one letter moves. Caret to offset and back at each row's two ends and middle, 11,037 checks: 5 differ, each inside a ligature, as they do without the toggle. Typing with the toggle on: 15.4 ms median, 28.7 ms p95, 360 keys, against 14.9 and 27.7 ms with it off in the same session.
- **Known limits:** drop capitals are not shown, so their lines look short. Headings, tables, code, and footnote text stay on toolkit lines. A page reference is set as "page 1" in this pass. Quotes and lists wrapped by hand in the source keep those breaks, since only a paragraph's lines are joined. Identical paragraphs share an entry.

**Spell checker.** `spell::Checker` is a small trait, with `NSSpellChecker` through `objc2-app-kit` (MIT, Apache-2.0, or Zlib; `objc2`, `objc2-foundation`, and `block2` are MIT, all already in the lockfile through gpui) and a `NoChecker` for other platforms.

- **Threads.** AppKit's header documents one background path, `requestCheckingOfString:...`, whose completion handler runs "in an arbitrary context". It says nothing on calling the synchronous methods off the main thread, though they worked when tried. So requests and suggestions are made on the main thread, and all checking happens on AppKit's queue. The answer comes back through a channel. Nothing happens on a key except restarting a 500 ms timer.
- **Language.** That path has no language argument, and an orthography in its options is ignored while the checker identifies languages itself, tried here in both directions. So the process's checker has identification off and the document's `lang` set, which is not kept after the app quits. Consequence: English quotations in a German book count as misspelled.
- **What is left out:** code blocks and spans, tables, directives, comments, front matter, images, citation keys, cross-references, footnote labels, link targets, URLs, words with `@`, `{...}` groups, and all markers. Markers are removed, not blanked, so `**fet**t` is checked as one word. Tables at rest show as a grid, so they have no underlines yet.
- **The word list is a plain file,** `druck-words.txt` beside `druck.yaml` or beside a single file, one word per line, compared without regard to case, read again when it changes. The checker's API would have let the app teach the system dictionary or a document tag. The file was chosen because a book's names belong with the book, can be edited and versioned, and leave the system alone.
- **Tried on real text:** 239,392 characters of the book in 1.9 s synchronously: 242 distinct words flagged, mostly names, Latin, and footnote labels the crude test did not mask. Umlauts and common compounds pass (`Gärtnerhäuschen`, `Größe`). Long compounds the dictionary does not know are flagged (`Softwareentwicklerkonferenz`, `Donaudampfschifffahrtsgesellschaft`, `Johannesevangelium`), and the word list answers them. I did not add a compound splitter on top of the system checker. Typing: 14.9 ms median with the checker on, against 15.7 ms measured on the gate branch before it, in another session.

**Accessibility.** gpui-pre 0.3.11 carries a real AccessKit tree on macOS: `.role()`, `.aria_label()`, and friends on elements with an id, with `accesskit_macos` behind the window. No parallel tree was needed.

- `a11y::Control` gives a control its role, label, tab stop, and focus ring. Enter and Space click a focused element in gpui already. Tab order runs through the title bar, the sidebar's one stop, the galley, the pages, the panel, and the rail.
- Read back here with the Accessibility API from a small Swift tool: buttons, check boxes, text fields with their values, a tab group, the contents list with each chapter's title, and an `AXTextArea` for the galley. `AXNumberOfCharacters`, `AXSelectedTextRange`, and `AXStringForRange` give the text on screen and the caret. Tab and Space were driven by keys and reached the canvas switch and the export button.
- **A gpui limit, worked around:** a cached view that is reused reports no nodes for that frame, so the sidebar and inspector dropped out of the tree. They are not cached while `Window::is_a11y_active`, at the cost measured when caching came in, 15.1 and 17.3 ms median. It costs nothing unless assistive technology is connected.
- **Gaps:** the galley reports the lines on screen only, one run per line, without character positions, so VoiceOver's reading of its bounds is a guess. A multi-line text area has no `AXValue` through AccessKit. The pages are one group labelled "Pages", with no text inside. Theme panel fields are labelled by their placeholder or not at all, because their visible label stands apart from the field: `TextInput::with_label` is there and only the Details fields use it. Chapter rows report as plain groups on macOS, as `Role::ListItem` maps. Nobody has run VoiceOver: that is for a person. The window's AX title is the whole path.

**Keyboard reach.** Added: Control+Command+1 to 7 for the inspector tabs (Agent is 7, which closes the release's missing way to reach it), Command+Option+0 to return to the text, Tab and Control+Tab, Command+Return and Option+Up or Down in the sidebar for a row's edit dialog, adding to the book, and moving, and a View menu with the tabs. Grep finds a role and a tab stop on nearly all 144 clickable controls.

What still takes a mouse: a block's convert menu and drag in the gutter (the slash menu and cut and paste do the same work), a hidden comment's marker, a click on a page, and a dragged file. Focus does not scroll a panel to the control it lands on, and the settings card does not trap Tab inside it. A row's hover buttons have keys in the sidebar but are not tab stops of their own, and the list of diagnostics is one tab stop per row, 243 on the book, with no arrow keys.

**The soak.** Scripted edits on a copy of the book as a project, in a fat-LTO bundle built with `measure`, keys by PID at about 130 words a minute. Two runs, 3,700 s each. Each round goes to a chapter by the sidebar's keys, finds a unique phrase with the find bar, types there, pauses, sometimes types and undoes a word, sometimes writes a footnote with its popover. One thread writes a word at the end of a long line every 5 to 11 s, half the time in the chapter in the galley, 70 percent by rename, 15 percent by delete and create, 15 percent in place, from reads up to 2 s old. It stands in for an agent. The theme file is written every 150 s, an export runs every 300 s by Tab and Space, and print lines are on for the middle third. The driver is not committed.

- **The first run found a real loss.** 11 of 43 footnote texts were gone, each with the placeholder left in the file. An outside change to the active chapter went through `adopt_active`, which closes what floats, and what was typed in the popover's field had not been written to the buffer. Grids and figure cards have the same field. Now `EditorView::holds_outside_changes` makes persistence wait while a float has the keys, as it does for an input method, and a chapter switch, closing, and quitting write the float first. Tried live for each path: the text was in the file every time. Nothing in the unit tests covers this, since it needs a window.
- **The second run, on the fix,** `a28a08d`: 18,777 keys, 326 typed passages, 419 outside writes, 38 footnotes, 117 chapter switches, 103 undos with 29 redos, 22 theme writes, 11 exports all landing. After Command+Q, exit 0, and each passage and word present exactly once. With them removed, every chapter equals the original byte for byte, except for what follows from undo. No text lost.
- **Undo stops at an outside change, by design:** the first Command+Z that reaches one does nothing. So 4 undone words stayed whole, and 5 stayed in part where a merge split their typing group. Nothing was lost by that, but it will surprise someone whose agent writes while they undo.
- **Typing, key handler to end of paint:** 1.9 ms median, 4.7 ms p95, 20.6 ms maximum, 18,341 keys, with no trend across the hour (1.7 to 1.9 ms after the first ten minutes' 3.1 ms). This does not count the wait for the next frame. The gate does, as a key timer through paint: 14.9 ms median at the start, 15.0 ms on the text the soak left, p95 31 ms.
- **Last change to painted pages:** 489 ms median, 538 ms p95, 585 ms maximum, none over 1 s, 915 samples. It rises with the text, from 469 to 520 ms, as the book grew by 8.4 percent.
- **Live heap: 171 MB at the first minute, 190 MB at the end, median 185 MB,** with spikes to 217 MB during layouts that fall back. By window: 173, 180, 185, 188, 190 MB. The rise slows but does not stop: 14 MB in the first half hour, 5 MB in the second. Not flat to the byte, and not attributed. The text itself explains little, about 3 MB between fresh opens of the start and end text. Undo histories in 17 chapters, never trimmed, are my first suspect. Resident 482 to 806 MB with no trend, as the allocator keeps freed pages. The gate's own heap read 180 MB in the morning and 85 MB after the soak on the same build settings, for a reason I did not find, so its absolute value should not be compared across sessions.
- **Crash reports:** none new, 8 before and after, all from 2026-11-03. No process left. Two recovery copies stayed after the clean quit, each with `text` equal to `base`, older than the file. Reopening with them changed nothing.

**Insert menu.** Footnote, citation, cross-reference, image, and table, on the galley's existing actions, with a new Command+Option+T for the table. A menu item with no galley behind it does nothing, as Undo does.

**`Cmd+Q after a keyboard-dismissed alert did not quit`: not reproduced.** Seven paths tried, with the app active and in the background: the About alert with Return, the failed-save alert with Escape, with Try again then Cancel, from the launch screen, with a field blurred, and with the settings card. Each quit, and each kept unsaved text as a recovery copy. With an alert still open, Command+Q does nothing, which is what a question about unsaved text should do. So there is no code change. If the owner sees it with a real keyboard, it is on the checklist.

## 2026-11-05: Writing-first direction

Settled with the owner in an interview, choosing from [the mockups](mockups/). The contract is [the desktop briefing](DESKTOP_BRIEFING.md), and the look is in [the design document](DESIGN.md). This replaces the three-pane composer with a live preview, and the app as it stood is the base. Nothing had been published, so the rework comes before any public beta.

- **Audience:** long-form authors first, replacing "books, reports, articles, and offers equally". Report and offer starters leave the app. The engine and CLI keep everything.
- **Two modes, no split:** page checking interrupts writing, so pages become a step. Authors who liked checking pages while drafting lose that; watch for it in beta feedback.
- **Bare page by default, Binder on a key:** the owner wanted both. A live toggle that is remembered avoids a second preference. A persistent list alone and a rail in both modes (mockup C) lost, since each keeps chrome beside the text.
- **Theme type on the app surface:** the kept sheet (G2) lost, since a white rectangle remains in dark mode. This reverses "App chrome never styles document content" for Write only.
- **Publish:** spread and rail (P1) with the contact sheet (P3) as the far end of one zoom scale. A separate proof queue layout (P2) lost as the main view, since a clean book would feel like a to-do list. It stays as the proof tab.
- **Proof slip:** chosen over editing on the page, where lines jump under the caret and input methods are hardest, and over jumping to Write, which would not meet the request. Layout help is limited to existing directives, so "no per-block overrides" holds. A one-line tighter or looser control was declined.
- **Write reports only what writing can fix:** a real book had 243 loose-line warnings, so dots on chapters said little. Print lines leave with this, since Publish answers that question.
- **All 30 theme panels stay, for 1.0:** behind a searchable index (S1), which beat an accordion (S2) and a group strip (S3) at this width. A smaller set of controls was offered and declined. The beta ships the sections authors use most, and JSON for the rest.
- **Agent as a writing partner:** off until enabled, on `Cmd+J`. Margin notes with chat on call (M1) beat a docked column (M2) and threads on passages (M3).
- **Agent edits land in the files:** the owner first chose proposals beside the text, then asked for IDE-style work on whole chapters and selections. Keeping edits in files means the CLI's normal tools, `CLAUDE.md`, skills, and MCP work unchanged, and the existing hunk tracking does the review. A shadow copy lost: more to build, and it drifts from what is being typed. Only advice stays beside the text.
- **Two tiers of history:** local history for small changes, JetBrains style, kept outside the project, and Git for deliberate versions. Snapshots as hidden Git refs were proposed and declined, since the owner wants small changes out of Git entirely. This reverses "No built-in Git" and "The app makes no commits"; the app commits only on request.
- **Git's own words:** commit and branch, chosen over version and draft. It narrows the audience a little and is a string table change if beta authors struggle.
- **`Cmd+S` stays a save:** writers press it from habit, so a card on it would interrupt them. The commit card is on `Cmd+Shift+S`. `Cmd+Shift+G` is macOS's Find Previous, so the Git panel takes `Ctrl+Shift+G`. `Cmd+K` stays on Link, so the palette is on `Cmd+P` and `Cmd+Shift+P`.
- **gitoxide plus the installed `git`:** gitoxide does not need Git on the machine, keeping the codebase Rust, unlike libgit2. It had no push as last checked, from a discussion written against 0.85, so push and pull call `git`, which also brings the author's keys. Check again before building.
- **Settings:** one user-level `settings.json` after Zed's window. A per-book scope lost, since what belongs to a book is already in `druck.yaml`.

Unresolved: Anthropic's answer on subscription logins, which no longer blocks a release since the agent is off by default and setup names an API key. How local history identifies a moved file. Whether proof slip reflow feels calm, since the mockups are HTML.

## 2026-11-05: Writing surface

Milestone 18. What the briefing and [the design document](DESIGN.md) left open.

- **Where things live.** `settings.json` stands in the user's config folder beside the common themes (`~/.config/druck/`), since that is where a person looks for it and common themes are already there. What the app only remembers, the window, recent documents, and zoom, stays in `state.json` in the app data folder. A choice someone would write down is a setting; a place the app came back to is state. Zoom is state because it follows the eye, not a preference. The Binder is a setting because the design says Settings shows it.
- **`settings.json` is strict JSON.** A comment makes it invalid, and that is reported rather than quietly stripped, since a tolerant reader would need a tolerant writer. Keys are snake case after Zed's. Unknown keys are kept, for a later version's settings. Only what differs from the default is written, one member at a time through `theme_file/patch.rs`, so what was written by hand keeps its bytes. A value back at its default leaves the file. An invalid file keeps the last good settings, the window and a line at the foot say why, and a choice made meanwhile holds for the run without touching the file.
- **Adding a setting** is a field and a line in `settings/file.rs::FIELDS`, and a row in `settings/window.rs::ROWS`. The categories are Appearance, Writing, Language, and Keys. Publishing and Agent wait for milestones 19 and 20, so no category stands empty. The mockup's interface size was left out: the tokens are constants, and nothing asks for it yet.
- **The long pause is 2 seconds.** Under that, a writer stopping to think would start a layout they cancel with the next word. Entering Publish asks for pages at once if they are behind. While the pages are off screen, `PreviewView::ready_ahead` rasters what Publish would show, so its first frame is exact: 15 ms on the book. It costs a raster of the visible pages and two on each side per pause, bounded by the page cache. Pages evicted meanwhile are freed from the raster's completion, since no frame comes to free them: the first version kept each one, and the gate showed the heap rising 2 MB per refresh, 213 to 422 MB over 100. With the fix it stays at 177 to 178 MB.
- **Theme type.** `preview::job::BookType` carries body, six heading levels, quote, list, footnote, and code: family, size, weight, italic, line height, space before and after, and first-line indent, plus the measure. Pixels per point stay 1.4, times the zoom. The room between blocks is the larger of the upper's space after and the lower's space before. A paragraph that follows a paragraph takes the first-line indent, as `src/layout` does. Paragraphs with neither space nor indent keep a small gap, or they would run together.
- **What Write does not show, on purpose or by limit.** Colours, justification, hyphenation, and pagination are the design's limits. These are the toolkit's or later work's: tracking, capitals, and small capitals, since gpui's text runs carry none of them; custom styles such as `{.copyright}`, which would need the attribute read per block; and a chapter's number above its heading, which the mockup shows and the engine alone knows. None of them moves a line break by much, and Publish shows them.
- **A first-line indent is broken by hand.** gpui wraps every row alike, so an indented paragraph is shaped once, broken at the last space that fits, and its glyph positions shifted, which keeps the caret's lookups working. Paragraphs with no indent use gpui's wrapping unchanged. This is what remains of the print lines' `force_rows`.
- **Dimming re-shapes, it does not paint with opacity.** `Window::with_element_opacity` is `pub(crate)` in gpui-pre 0.3.11, and a shaped line holds its colours. Colour is outside gpui's line layout cache key, so moving between blocks costs two cheap shapes, and typing inside a block costs nothing. Typing with both focus features on is under 16 ms.
- **Typewriter scrolling follows keys, not clicks.** The caret's line stands at 45 percent of the pane's height. A click leaves the text where it is, because it would otherwise jump under the pointer and start a drag in the wrong place. The view may go past the chapter's ends just far enough for the first and last line to reach that height.
- **Margin notes** are 220 px wide, scroll with the text, stand beside the chapter's first text, and are edited in place. Escape returns to the text. They replace the Notes tab and the hidden-comment marker that pointed to it. The text moves left for them only when the window cannot hold both, then narrows, and in a window under about 530 px they do not show; the source view still has them.
- **The word count** reads "2 251 of 37 508 words": one faint line holding the chapter and the book, which the old footer gave as a sentence. The zoom's value shows in the same place for a moment.
- **Dots mean errors and placeholders.** `Report::marks` no longer counts warnings, closing the follow-up about 243 loose-line dots. The dot for an error is the accent, as the design says.
- **Moved early, because the title bar had to empty.** The theme picker went to the top of the Styles tab, and Publish's Export button went into the title bar. Milestone 19 lists both, and can keep or redesign them. The source view's button went: `Cmd+/`, the View menu, and the palette have it.
- **Removed outright:** Split, the sheet, the galley's header, the footer strip, the caret line on pages (only proof marks appear on pages; pages still scroll to the caret), the inspector toggle, and print lines, engine included: `editor::composer`, `pipeline::set_lines`, and `tests/composer.rs`. Fit now fits a whole page, since a wide pane with no split made fit-to-width 300 percent.
- **The palette lists commands in the app's language only,** and leaves out in Publish what needs the text on screen. Its table in `shell/commands.rs` also feeds the settings window's Keys list, so a key shows in one place. A test keeps every menu item in it.
- **`Cmd+\` on a German keyboard** needs Option and Shift for the backslash, so `Cmd+Option+Shift+7` may be what reaches it. It is bound as the design says, and the menu and palette also have it. For the owner to try.
- **Debug hooks** replace `DRUCK_CANVAS` and `DRUCK_GATE_PRINT`: `DRUCK_MODE`, `DRUCK_SETTINGS`, `DRUCK_PALETTE`, `DRUCK_TICKS`, `DRUCK_GUTTER`, `DRUCK_CHAPTER`, and `DRUCK_SELECT`. All go through `gate::var`, so a shipped build has none. `XDG_CONFIG_HOME` and `DRUCK_DATA_DIR` keep a measurement away from the owner's settings and state. A stale data folder restores unsaved text as recovery copies, so a scripted run should start with an empty one.

## 2026-11-05: Writing partner

Milestone 20. What the briefing and [the design document](DESIGN.md) left open. `desktop/src/agent/` is the agent, and `desktop/src/editor/review.rs` is what the text draws of it.

- **The margin-note tool is a local MCP server in the app's own executable.** The installed CLI's help (2.1.318) documents `--mcp-config <configs...>`, which takes JSON strings for one run, and `--allowedTools`. So the CLI starts with `--mcp-config` naming `current_exe() --agent-note-tool`, `--allowedTools mcp__druck__leave_note`, and `--append-system-prompt` saying what the tool is for. All three last one session and touch no settings file, and without `--strict-mcp-config` the author's own servers still load. Writing a file with the CLI's normal tools lost, since a model gets a JSON file wrong and the author would be asked to allow it. A permanent `claude mcp add` lost, since it changes the author's settings. The SDK's in-process `sdk` server type lost, since `--help` does not document it. `agent/note_tool.rs` is about 100 lines of JSON-RPC over stdio. A live turn called it with no approval asked.
- **The app is the one writer of `.druck/notes.json`.** The server only checks a note: the fields are there, the path stays in the project, and the quote is in the file, and if not the model is told and tries again. The app takes the note from the tool call in the CLI's stream. So there is nothing to merge and nothing to watch.
- **A quote is found by exact text, first match.** A note whose quote is gone stands at the top of the margin, says so, and offers only Dismiss. Quoting whitespace or marks differently than the file is refused at the tool.
- **Removed words are zero-length chips, so nothing about them is in the buffer.** A `Chip` with an empty source range shows text that stands for no source, which `DisplayLine`'s map already handles: the caret, clicks, and every offset keep to the Markdown. `ChipLook::Removed` strikes it through on the `removed` tint. New words are tinted rectangles that start past the chip. Word changes come from `merge::edits_between`, and changes fewer than three words apart join, or a reworded sentence reads as confetti. A removal with no text left to stand in moves to the next line with text. A paragraph break in removed text shows as a pilcrow.
- **Passages are hunks, and a new chapter is cut into them by blocks.** Text that only came in is one hunk per block with its blank lines. A comment stays with the block after it, since it shows nothing to review by. So a chapter arrives as one passage after another, and a rewritten chapter already does, since unchanged blank lines separate hunks.
- **Review does not hide while typing any more.** Milestone 16's marks hid on a key and came back 150 ms later. With room for buttons and struck words that would make the text jump on every pause, so the galley moves what it shows with each change itself (`review::shift`), and the agent module looks again 150 ms after the text rests.
- **A reject replaces the hunk's lines, and no longer goes through `three_way`.** This replaces "A revert is a merge" in the [integration entry](#2026-11-04-desktop-beta-integrating-slices-2-to-6). A reject only runs on lines found exactly as the agent left them, so the merge could add nothing there and could only refuse. It refused rejected neighbours in a new chapter, whose hunks touch. The replaced bytes come from the `base` the change started from, so line ends and a missing final line break survive. A reject is one undo step, and an undo puts the passage back under review, since a rejected hunk stays tracked.
- **A reject disowns the file's earlier versions (`Session::disown_versions`).** Persistence measures a change from the earlier version closest to it, so an agent trying again after a reject was merged against its own rejected text, and part of that text came back. The live Try again found this. A write already under way when the author rejects can still bring it back, which then shows as a new change.
- **Reject all returns exact bytes.** A step of changes the author did not touch goes back to its `base` whole, newest first, so no diff is involved. Where the author typed, it goes hunk by hunk and keeps their words. A file the agent made goes when nothing is left in it, and its line in `druck.yaml` goes first.
- **A passage the author edited is theirs.** Its lines are no longer as the agent left them, so it leaves review and the count, stays in the text, and comes back if they undo. Offering a reject there would mean guessing what is theirs. Reject all leaves it too. This is the one way an agent's change stops being counted without a decision, and it is on purpose: the author has read it.
- **Accept moves a hunk into its change's `base`.** So a later write recomputes the rest without bringing it back.
- **Accept all and Reject all act on every file with changes,** and the foot says how many stand elsewhere, with Next. The mockup shows one chapter, but [the criteria](milestones/20-writing-partner.md#completion-criteria) ask that rejecting everything restores the bytes of a turn across chapters, and a button that leaves other chapters changed would not.
- **The manifest and new files have no `FileChange`, so the agent module looks for them.** Persistence only reads `druck.yaml` when no chapter holds it, and a new file is only discovered. At each session change during a turn the disk's manifest is compared with the last look, and a file new to the session counts as made by the turn if it was created after the turn began. Same limit as before: any outside change during a turn counts as the agent's.
- **`agent::unreviewed_changes(cx: &App) -> Vec<Unreviewed>`** is the seam for export and commit: chapter, path, name, and count per file, as of the last time the text rested, and empty with the agent off. They name these and do not block. The integrator wires them.
- **`label_local_history_before_turn` in `agent/mod.rs` is where milestone 21's label belongs.** It is empty, and the turn's own hunks undo a turn until then.
- **Off means off.** `settings.agent`, default false. The rail's tab, the menu items, the palette's commands, the Keys rows, the foot, the notes, and `Cmd+J` all check it. The first CLI lookup waits until `Cmd+J` or the rail's tab, and turning it off ends a running process. One line in `inspector/mod.rs` hides the tab, in a file milestone 19 owns, because the tab's mount had to stay and the tab must not show.
- **The conversation is one view drawn in two places.** The rail's tab mounts `AgentView` as before, and the chat card draws the same entity over the text in Write. They are never on screen together, since the rail only shows in Publish. So the docked panel left as a place and not as code: its rows and approval card are the card's.
- **New conversation keeps what is under review,** since it is in the files whichever conversation made it.
- **Review is not kept across a restart.** Hunks live in memory, so after a restart the text stands as the agent left it, unmarked. Milestone 21's labels are the way back then. Notes are on disk and do come back.
- **What a selection prompt sends.** The chapter and the selected text, with one line saying to change nothing around it, and not the diagnostics, which are about the book. The selection collapses to a caret when it is sent, so a key cannot type over text about to change.
- **Debug hooks.** `DRUCK_AGENT_SCRIPT` names a file of actions (`agent/probe.rs`), through `gate::var`, so a shipped build has none. `DRUCK_GATE=0` floats the window without the measurement, which keeps it painting behind other windows.

## Recording a decision

Add a short dated entry when a choice affects future work. State the choice, reason, affected interface or behavior, and any unresolved consequence. Link to code, schema, or tests once they exist. Replace superseded guidance with a reference to the newer decision.

Keep dependency versions and runnable commands in their actual manifests and tooling. Do not copy them here. Record why a dependency was chosen when that reason is not evident from the code.
