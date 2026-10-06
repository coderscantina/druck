# Progress

## Current state

Milestone 01 is complete. The CLI validates a document's front matter, one theme, and `--set` overrides, resolves them into typed configuration, and reports diagnostics. It does not render PDFs.

Current milestone: none active.
Next milestone: [02: First text-to-PDF rendering path](milestones/02-text-to-pdf.md), when implementation is requested.

## Milestone status

- [x] 01: Foundation and theme contracts.
- [ ] 02: First text-to-PDF rendering path.
- [ ] 03: Optimized paragraph composition.
- [ ] 04: Scored pagination and footnotes.
- [ ] 05: Mid-page column layouts.
- [ ] 06: Images and captions.
- [ ] 07: Multipage tables.
- [ ] 08: Document structures and templates.
- [ ] 09: Citations and bibliography.
- [ ] 10: Release acceptance and distribution.

## Resume note

Configuration lives in `src/config/` and is consumed as `resolved::Config`. The CLI in `src/main.rs` reads files and checks resource existence. Choices are in [decisions](DECISIONS.md), user docs in [authoring](AUTHORING.md) and [themes](THEMES.md). Milestone 02 starts by adding the Libertinus font files under the bundled origin, confirming their license and German coverage, and replacing the `render` placeholder.

## Verification

On 2026-10-06, macOS: `cargo test -q` passed 21 unit and 11 integration tests. `cargo clippy --all-targets -q` and `cargo fmt --check` were clean. The integration tests run the binary from a separate working directory and cover theme, document, working-directory, and bundled origins, four-layer precedence, merge and null rules, tokens, units, versions, unknown fields, YAML line numbers, missing resources, and the unavailable `render`.

Not yet run: the CI workflow on Linux and Windows, since nothing has been pushed.

## Blockers and follow-ups

- Required title slots are not checked yet. Metadata placeholders could be checked now, but which title layout is active is a milestone 08 decision.
- `--set date=2024` parses as a number and fails; quoting works. Consider accepting numbers for text metadata.
- Front matter font settings take effect only if the theme uses the `body`, `heading`, and `mono` font tokens. A warning for unused settings might help.
- Paths in diagnostics are absolute and not normalized (`doc/../theme`).
- Displayed page numbering, `{section}` selection, and required-slot timing are open for milestone 08.

## Updating this file

Replace the resume note with the latest completed work and next action. Record exact verification commands and results, remaining criteria, and blockers. Check a milestone only after its completion gate passes. Preserve useful follow-ups without copying the product requirements or decision history here.
