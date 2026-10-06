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

Fenced containers in the style of Pandoc divs, documented in [authoring](AUTHORING.md): `::: columns`, `::: full-width` (only directly inside `columns`), and `::: keep`, each closed by a line of colons. `::: page-break` stands alone. Directive lines inside code blocks are code. The parser arrives with the features that use it. Caption, label, cross-reference, and citation syntax is decided in milestones 06, 08, and 09.

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

[Line filling](../src/layout/paragraph.rs) is greedy, word by word, with justified lines stretched evenly and no hyphenation. A word wider than its line is an error. Pages break at the first line that does not fit. Milestone 03 replaces line filling and milestone 04 pagination; neither is counted as completed typography.

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

## Recording a decision

Add a short dated entry when a choice affects future work. State the choice, reason, affected interface or behavior, and any unresolved consequence. Link to code, schema, or tests once they exist. Replace superseded guidance with a reference to the newer decision.

Keep dependency versions and runnable commands in their actual manifests and tooling. Do not copy them here. Record why a dependency was chosen when that reason is not evident from the code.
