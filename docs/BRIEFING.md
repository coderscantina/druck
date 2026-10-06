# Briefing: Native Rust Markdown-to-PDF renderer

## Goal

Build a local command-line renderer that turns simple Markdown into carefully typeset PDFs for articles, reports, and short manuscripts.

The ambition is the typographic quality associated with LaTeX: well-composed paragraphs, deliberate page breaks, consistent document structure, and restrained scholarly presentation. Markdown remains the authoring language, with a small number of additions for document settings and layout intent.

The renderer owns a native Rust layout engine. It has no TeX dependency. Existing libraries may provide Markdown parsing, font processing, text shaping, image handling, and PDF output.

This briefing defines phase 1: the complete CLI release, including the JSON template/theme architecture. Delivery within this phase is staged; staging does not remove requirements from its scope.

Phase 2 provides a public Rust crate for native client integration. Its contract is specified separately in [Rust crate and API: Phase 2](RUST_CRATE_API_PHASE_2.md).

## Audience and usage

The first release serves authors writing prose-heavy articles, reports, and short manuscripts in English and German.

The primary interface is a local CLI that accepts a Markdown document and produces a PDF. Rendering works fully offline on macOS, Linux, and Windows.

An editor and live preview are outside the first-release contract. The public Rust library API is a phase 2 deliverable. Phase 1 should keep CLI concerns separate from the rendering engine so phase 2 can expose the same behavior without creating another implementation.

## Authoring contract

Use CommonMark as the Markdown baseline, with an explicit, documented set of extensions:

- Tables.
- Footnotes.
- Citations.
- Captions and labels.
- Cross-references.
- A small set of layout directives.

Use YAML front matter for document metadata and settings. Expose a limited set of document-level controls for fonts, page size, margins, spacing, and the gap between columns. Metadata also covers the selected document structures and bibliography settings. Document settings cannot redefine template structures.

Use a separate JSON theme for reusable document design. It configures supported element styles and structured title-page, header, and footer templates. The author-facing document settings remain small even though theme authors can configure the supported design surface more fully.

The layout directives express intent: explicit page breaks, keep-together groups, changes between one and two columns, and full-width blocks within a column section. Do not expose arbitrary coordinates or a general typesetting command language.

Exact directive spelling and the settings schema are implementation decisions. They must remain small, documented, and consistent. Unsupported constructs receive clear diagnostics. Embedded HTML is not rendered.

## JSON template/theme architecture

### Format and responsibilities

Use one versioned JSON format containing both styles and structural templates. Bundle a complete default theme that produces the scholarly design described in this briefing. A selected theme may be partial.

Themes describe supported visual settings and document structures. They cannot disable layout correctness guarantees, introduce arbitrary coordinates, or replace the layout algorithms. Do not provide a programming language, CSS-like selectors, cascading style rules, general expressions, or arbitrary element trees.

### Resolution and overrides

Resolve configuration in this order, with later explicit values taking precedence:

1. Bundled defaults.
2. One selected JSON theme.
3. Explicit document settings from YAML front matter.
4. Explicit CLI overrides, or caller overrides when the phase 2 API is available.

Merge objects recursively by field. Replace arrays as a whole. An omitted field inherits its earlier value. Reject null unless the schema explicitly permits it for that field. Provide explicit supported values to disable optional features.

Support neither chains of parent themes nor multiple ordered theme overlays. Document settings retain their limited surface and cannot redefine templates. All supplied layers must be valid; a later override does not excuse an invalid earlier value. Validate the resolved configuration as well as individual inputs.

### Tokens, styles, and measurements

Provide named tokens for fonts, sizes, spacing, and colors, and explicit styles for each supported element. This includes body text, headings, lists, quotations, code, captions, tables, footnotes, and bibliography content.

Token references must resolve unambiguously. Diagnose missing references and reject cyclic references if the schema permits tokens to reference other tokens. Exact token identifiers and style properties remain implementation decisions.

Support explicit lengths in pt, mm, cm, and in. Support em for typography-relative measurements and a separate unitless line-height multiplier. Document the font-size basis for em in each allowed context. Do not support viewport units, general calculations, or percentage-based measurements.

### Structured templates

Title-page templates contain ordered content slots. Header and footer templates have fixed left, center, and right slots. Slots have supported styles and bounded spacing.

Slot values can refer to document metadata, applicable section titles, and page numbers. Insert values as text, never as layout commands or executable expressions. Omit empty optional slots and their associated spacing. A missing value for a slot marked required is an error.

Support explicit page variants for title pages, the first body page, normal body pages, odd pages, and even pages. Define a deterministic fallback order and document which page-numbering sequence determines parity. Variant precedence, section-title selection, and empty-slot behavior must be consistent across page layout and final PDF output.

Nested row/column template containers, arbitrary selectors, conditional expressions, and absolute positioning are outside scope. Body columns retain the explicit one/two-column behavior specified below.

### Validation and resources

Publish a JSON Schema and represent the configuration with typed Rust structures. Include an explicit schema version. Reject unknown fields, unsupported versions, invalid values, and invalid combinations with clear diagnostics. Identify the theme source and offending property where possible.

Theme font and image assets resolve relative to the selected theme's origin. Document images and bibliography files continue to resolve relative to the Markdown document's origin. Do not let configuration merging accidentally rebase a resource reference onto a different origin. Bundled resources retain their bundled origin.

Theme loading is local and offline. Document how to distribute a theme together with its referenced assets. A theme is JSON plus any local assets it references; archive packaging and network registries are not required.

Exact JSON property names, slot identifiers, page-variant fallback order, measurement encoding, and configuration type names remain implementation decisions within these rules.

## Supported content

The first release typesets:

- Paragraphs and headings.
- Ordered and unordered lists.
- Block quotations.
- Inline code and fenced code blocks.
- Images and captions.
- Tables with wrapped cell text.
- Footnotes.
- Citations and bibliographies.

Math is outside the first release.

## Paragraph composition and typography

Optimize line breaks across each paragraph rather than wrapping each line independently. Composition must use the actual shaped text metrics and account for available width, word spacing, hyphenation, and the quality of the paragraph as a whole.

Required typography includes kerning, ligatures, English and German language-aware hyphenation, and optical margin alignment. The first release supports left-to-right text in these two languages.

Use a restrained scholarly default: serif body text, justified paragraphs, generous margins, clear heading hierarchy, and quiet captions and footnotes. Authors should obtain a polished result without tuning individual blocks.

Default fonts and numeric spacing constants are implementation decisions within this direction. Bounded glyph-width adjustments for justification are deferred.

## Page composition

Score candidate page breaks using content constraints and spacing quality. Account for the actual height of paragraphs, headings, lists, quotations, images, captions, tables, and footnotes.

Keep headings with following text and avoid single paragraph lines stranded at the bottom or top of a page. Respect explicit page breaks and keep-together groups. If a requested constraint cannot be satisfied, report the conflict rather than silently losing or overflowing content.

Use a consistent text area and bounded adjustment of vertical spacing to align page bottoms. Short final pages are allowed. Large spacing distortions are not an acceptable way to fill a page.

Paragraph optimization and page-break scoring are separate responsibilities that must cooperate. Joint global optimization of every line and page is not required.

## Mid-page column layouts

Authors can switch between one column and two equal-width columns on the same page. The gap between two columns is configurable. Unequal widths and more than two columns are outside scope.

In a two-column section, text flows down the first column and then down the second in reading order. A section may continue across pages.

At the end of a column section, balance its final columns and start the next layout below the tallest column. A layout change does not itself force a new page. If there is insufficient space, normal pagination moves subsequent content to the next page.

Images and tables use the current column width by default. An explicit full-width block balances the preceding columns, places the block in document order across the text area, and resumes the column layout below it. The block still follows the normal image or table pagination rules.

Column balancing must account for the page's footnote area, keep-together constraints, and other pagination rules. Do not balance columns by clipping content or introducing excessive spacing.

## Footnotes

Place footnotes in one full-width area at the bottom of the page, including pages containing multiple column layouts. Order them by their references in document reading order.

A footnote begins on the page containing its reference. Its height participates in page-break scoring and column layout. Long footnotes may continue onto subsequent pages with a clear continuation marker.

Footnotes are part of page composition, not an addition drawn after pagination has already been decided.

## Images and tables

Keep images in document order and with their captions. Move an image and its caption to the next page or column when needed. Proportionally downscale oversized images to fit the available layout area.

Support local PNG, JPEG, and SVG images. Automatic floats, wrapping prose around images, and automatic promotion to full-width placement are outside scope.

Tables wrap cell text, repeat headers across pages, and break between rows. They can continue across pages. A single row taller than the available page area receives an error.

Merged cells and splitting an individual table row across pages are outside scope.

## Document structure

Support the following structures and navigation features:

- Title blocks and optional abstracts.
- Separate title pages.
- Numbered headings.
- Optional tables of contents.
- Page numbers and running headers.
- Labels and cross-references to headings, figures, and tables.

Generated numbering, references, and page information must agree with the final layout. Resolve any pagination dependencies consistently before emitting the final PDF. Running-header and title-page styling come from the resolved JSON theme, with polished bundled defaults.

## Citations and bibliography

Read an external BibTeX bibliography and provide built-in author-date and numeric citation styles.

Support citation keys such as `[@smith2024]`, multiple keys, page locators, and narrative citations. Exact syntax for locators and narrative citations remains an implementation decision.

Support bibliography entries for articles, books, conference papers, theses, reports, and web resources. Define and document the required fields and formatting rules for supported entry types.

Missing citation keys and missing required fields are errors. Unknown optional BibTeX fields may be ignored.

Arbitrary CSL styles, custom entry types, and broad BibTeX compatibility such as macros and cross-reference inheritance are outside the first-release contract.

## Resources, diagnostics, and failure behavior

Resolve document resource paths relative to the input Markdown file and theme resource paths relative to the theme origin. Resources include local images, bibliography files, and optional local font files. Preserve the origin of each resource reference through configuration resolution. Do not fetch remote resources.

Bundle the default fonts and hyphenation data so ordinary rendering does not depend on system-installed resources.

Give clear diagnostics tied to the source location where possible. Errors must identify the relevant resource, citation, reference, or layout constraint.

After allowed proportional image downscaling, unbreakable content that still cannot fit receives an error. This includes oversized table rows and code lines that exceed the available width. Do not silently wrap code, shrink text, clip content, or emit a successful result with concealed overflow.

The CLI must distinguish successful rendering from a failed render. Detailed command names, flags, and diagnostic formatting remain implementation decisions.

## PDF output and reproducibility

Required PDF features are:

- Embedded fonts.
- Selectable and searchable text.
- Clickable links and cross-references.
- Heading bookmarks.

For identical document inputs, resolved theme and settings, resource contents, and renderer versions, layout must be identical across supported platforms. Custom fonts and theme assets are part of the input resources. Byte-for-byte identical PDF files are not required.

Tagged accessibility and PDF/A compliance are deferred.

## Phase 1 delivery stages

The stages below describe product delivery. [The ten development milestones](PHASE_1_MILESTONES.md) divide them into bounded agent work with explicit completion gates. Current implementation state is tracked in [progress](PROGRESS.md).

### Stage 1: Core composition

Implement the Markdown content model, versioned theme schema, typed configuration, validation, token resolution, override resolution, bundled default theme, text shaping, paragraph optimization, required typography, basic single-column page composition, and PDF text output. Establish reproducibility and useful source diagnostics early. Keep CLI input/output separate from rendering responsibilities.

Benchmark the first working layout prototype before setting runtime and memory limits.

### Stage 2: Page and column constraints

Implement mid-page column changes, column balancing, explicit full-width blocks, keep-together behavior, images, captions, multipage tables, and footnotes with continuation.

Verify these features in combination, especially their competition for page space. An isolated column demonstration or footnote demonstration is insufficient.

### Stage 3: Complete documents and distribution

Implement citations, bibliographies, theme-driven title structures, tables of contents, numbering, cross-references, header/footer slots, page variants, and complete PDF navigation.

Finish CLI distribution for macOS, Linux, and Windows, bundled resources, and the full acceptance corpus. All stages together constitute the agreed first release.

### Phase 2 handoff

Expose this renderer as a public Rust crate after the phase 1 contract is complete. Phase 2 adds the supported API, inspectable layout results, explicit resource resolvers, cancellation, concurrent job contract, and CLI integration through the public API. See the separate phase 2 briefing for its requirements and acceptance criteria.

## Acceptance and performance

Judge visual quality against the project's own reviewed English and German sample documents. LaTeX reference PDFs and matching LaTeX line or page breaks are not acceptance requirements.

The acceptance corpus must cover short prose, longer reports, and difficult combinations of supported features. Include mid-page one-to-two-to-one column changes, column sections continuing across pages, full-width blocks, footnotes on mixed-layout pages, long footnote continuation, and multipage tables.

Use automated checks for overflow, lost content, broken references, and unstable pagination. Verify reproducible layout across supported platforms. Check selectable text, links, bookmarks, and embedded fonts in the resulting PDFs.

Verify theme behavior with the bundled default and at least one custom theme. Cover nested object merges, whole-array replacement, omitted fields, permitted and rejected null values, token resolution, override precedence, missing slot values, page variants, invalid schema versions, unknown fields, and invalid values. Verify theme-relative and document-relative assets even when the CLI runs from another working directory.

Include visual samples for custom title pages, headers, footers, and typography. Check that theme customization preserves the existing pagination, column, and footnote guarantees.

Visual review must assess paragraph spacing, hyphenation, heading hierarchy, page density, column balance, captions, footnotes, and bibliography presentation. A PDF that merely opens successfully does not satisfy the quality goal.

Benchmark 10-, 50-, and 100-page documents. Set runtime and memory limits after the first working layout prototype and carry them into release acceptance. No performance numbers are agreed yet.

## Explicit boundaries and remaining implementation decisions

The first release excludes math, automatic floats, image text wrapping, unequal or three-column layouts, merged table cells, split table rows, arbitrary typesetting commands, embedded HTML rendering, remote resource fetching, arbitrary CSL styles, broad BibTeX compatibility, glyph-width adjustments, right-to-left and CJK layout, tagged accessibility, and PDF/A compliance. Theme inheritance chains, multiple theme overlays, cascading selectors, arbitrary template trees, expressions, and absolute positioning are also excluded. The public Rust API is deferred to phase 2.

Dependency crate choices, internal module interfaces, exact directive spelling, the front-matter schema, exact JSON properties and slot identifiers, page-variant fallback order, measurement encoding, default font selection, numeric typography settings, concrete citation formatting, and CLI flags remain implementation decisions within this contract. These choices must be documented and deterministic. Performance limits will be set from prototype measurements.

The briefing authorizes no implementation by itself.
