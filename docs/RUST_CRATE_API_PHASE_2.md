# Briefing: Public Rust crate and API, phase 2

## Goal and relationship to phase 1

Provide a public Rust crate that lets native desktop applications and backend services integrate the renderer defined in [the phase 1 briefing](BRIEFING.md).

Phase 1 delivers the complete CLI renderer and JSON template/theme architecture. Phase 2 exposes that same engine through a documented, supported API. It does not introduce a second layout implementation or reduce the existing document, typography, pagination, column, citation, PDF, or reproducibility requirements.

Target native clients on macOS, Linux, and Windows. Browser/WASM support and stable C bindings are outside this phase.

This document specifies requirements. It does not authorize implementation or publication of a crate.

## Public API surface

Provide two entry paths:

1. A simple API that accepts Markdown, configuration, and resource resolution and returns PDF output plus structured diagnostics.
2. A layout API that returns an immutable, inspectable result, followed by PDF export from that result without repeating layout.

Document and support typed configuration, theme loading and resolution, resource resolution, diagnostics, cooperative cancellation, layout inspection, and PDF export. Clients must be able to render from in-memory inputs and consume PDF bytes without temporary files.

Keep parser internals, intermediate document structures, layout scoring algorithms, and implementation-specific caches private. An inspectable final layout is part of the supported API; exposing it does not require exposing the engine's internal representations.

Exact crate names, Rust type names, function signatures, and output buffer conventions remain implementation decisions. Prefer a small public surface that hides engine coordination while providing the agreed capabilities.

## Configuration and themes

Reuse the phase 1 JSON schema and typed configuration. Apply the same validation and resolution rules:

1. Bundled defaults.
2. One selected JSON theme.
3. Explicit document settings.
4. Explicit caller overrides.

Objects merge by field, arrays replace as a whole, omitted values inherit, and null requires explicit schema support. Document settings cannot redefine template structures. Themes cannot disable layout correctness guarantees.

Preserve each resource reference's origin through resolution. Theme assets are relative to the theme origin, document assets to the document origin, and bundled resources to their bundled origin.

The simple rendering path and explicit layout/export path must resolve identical inputs identically. Configuration errors must be reported before they produce invalid layout.

## Resource resolution

Obtain fonts, images, and bibliography data through an explicit caller-supplied resource resolver. Provide filesystem and in-memory implementations.

The core library performs no implicit filesystem or network access. A filesystem resolver accesses files only as part of the caller's explicit choice. The in-memory resolver allows a client to render without filesystem access.

Supply explicit document and theme origins when relative resources are used. Report failures with the resource identifier, applicable origin, and originating source location where available.

Bundled fonts and hyphenation data remain available without machine-specific font discovery. The crate itself does not fetch remote resources. Application-side resource acquisition is outside its responsibilities.

Resource resolution used by concurrent jobs must follow a documented concurrency contract. The renderer must not assume that a path or identifier always denotes unchanged bytes. Reuse preparation only when resource identity and content validity are established.

## Inspectable layout result

Return an immutable page display list with:

- Page dimensions and ordered drawing content.
- Resolved glyph identities and positions with the fonts required to interpret them.
- Graphics and image placement with resolved resource data.
- Links and their destinations, including cross-references.
- Source mappings connecting relevant positioned content to document input.
- Document navigation information required for PDF export.

The result is self-contained for rendering: it retains access to the resolved fonts, images, and other necessary resources rather than requiring the caller to fetch them again. Owned or shared immutable storage is acceptable. A result must remain usable after the original request inputs and renderer are released.

PDF export consumes this result without repeating text shaping or pagination. Client previews can consume the same resolved display list without reconstructing line breaks, font selection, or resource resolution.

Document the coordinate system, measurement units, drawing order, glyph interpretation, resource access, and source-mapping semantics. Do not expose backend-specific handles that prevent native clients from inspecting and drawing the result.

A built-in raster preview API, editor widgets, and application rendering adapters are not required. Clients own preview integration. The display list still must contain enough information to draw a faithful preview.

## Execution, reuse, and concurrency

Provide synchronous rendering and layout operations with cooperative cancellation. Clients own threads, scheduling, and async integration. Do not require an async runtime or provide a built-in job scheduler.

Define cancellation for resource preparation, layout, and PDF export, including clear cancellation outcomes. Cancellation must not return incomplete output as a successful render. A cancelled or failed job must not corrupt reusable renderer state or affect other jobs.

Provide a reusable renderer that retains reusable font and resource preparation while accepting separate per-document requests. Keep mutable job state isolated. Independent jobs can run concurrently without global mutable configuration or a global mutable renderer.

Document how the renderer, resolver, cancellation controls, and layout results can be shared across threads. Enforce the documented contract through Rust types rather than leaving clients to infer it.

Cache design and internal synchronization are implementation decisions. Preparation reuse must preserve the phase 1 reproducibility contract and respect changes in resource contents and configuration.

## Diagnostics and failure behavior

Return structured diagnostics that clients can present without parsing CLI text. Preserve document source locations, theme properties, resource identifiers, and relevant origins where available.

Distinguish successful output, warnings, fatal rendering errors, and cancellation. Retain the phase 1 behavior for unsupported syntax, invalid configuration, missing citations or resources, unsatisfied constraints, and content that cannot fit.

Diagnostics are part of the public API. Define stable meanings for their public categories and source references. Exact diagnostic codes and presentation text remain implementation decisions.

The crate must not terminate the host process or write unsolicited output to standard output or standard error. The CLI owns terminal presentation and exit codes.

## CLI parity

Refactor the CLI to use the public crate API for configuration resolution, rendering, layout, and PDF export. CLI-specific responsibilities are argument parsing, explicit filesystem resolver setup, input/output handling, diagnostic presentation, and exit codes.

For identical Markdown, resolved themes and settings, resource contents, and renderer versions, the CLI and an embedded client must produce identical layout. Byte-for-byte identical PDFs remain outside the contract.

Both the direct Markdown-to-PDF path and layout-then-export path must preserve this parity.

## Compatibility and documentation

Version the public Rust API and JSON schema separately. Document which schema versions each crate release supports. Unsupported schemas must fail clearly.

Treat typed configuration, resource resolution, diagnostics, cancellation, layout inspection, and PDF export as supported public interfaces. Keep implementation details private so changes to scoring, parsing, and cache internals do not unnecessarily break clients.

Provide documentation and compilable examples for:

- Simple rendering with bundled defaults.
- Loading a custom JSON theme and supplying caller overrides.
- Fully in-memory rendering with an explicit resource resolver.
- Layout inspection followed by PDF export.
- Cooperative cancellation.
- Reusing the renderer for concurrent independent jobs.

The API contract must be usable by both a desktop client and a backend service. A demonstration that only calls the CLI as a subprocess does not satisfy this requirement.

## Delivery sequence

1. Extract and document the supported renderer and configuration API while retaining the phase 1 behavior.
2. Provide explicit resource resolvers, reusable renderer state, isolated concurrent jobs, structured diagnostics, and cancellation.
3. Expose the self-contained layout result and PDF export from it.
4. Route the CLI through the public API and complete parity checks, documentation, examples, and release packaging.

Crate naming, dependency selection, and publication details remain implementation decisions. Actual publication requires a separate release request.

## Acceptance

Exercise the phase 1 document and theme corpus through the public API. Verify that simple rendering, layout-then-export, and the CLI produce identical layout for identical inputs.

Verify fully in-memory rendering, filesystem resolution with distinct document and theme origins, structured errors, required and optional slots, override precedence, and strict schema validation.

Verify that an inspectable result retains its fonts and images after its input buffers and renderer are released, and that PDF export does not repeat layout or resolve resources again. Check that the display list supports faithful native preview drawing and useful source inspection.

Verify concurrent jobs using different documents and themes without configuration leakage. Verify cancellation during meaningful work and subsequent reuse of the renderer. Confirm that changed resource contents cannot reuse stale preparation.

Run targeted API tests and compilable documentation examples on supported native platforms. Revisit phase 1 runtime and memory limits with reused renderer state and concurrent workloads. Set any additional concurrency limits from measurements rather than assuming unlimited parallelism.

## Boundaries and open implementation details

This phase excludes browser/WASM support, C bindings, an async runtime, built-in job scheduling, a raster preview API, editor components, arbitrary parser or layout plugins, and public mutable intermediate document editing.

Exact signatures, type names, diagnostic codes, coordinate representation, source-mapping encoding, resolver interfaces, cancellation mechanics, cache design, publication details, and release numbers remain implementation decisions within this contract. They must be documented and tested before the public API is released.
