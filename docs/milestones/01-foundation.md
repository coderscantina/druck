# 01: Foundation and theme contracts

## Goal and prerequisites

Establish the Rust CLI project and its validated input/configuration boundary. This is the first implementation milestone.

Read [the product contract](../BRIEFING.md) in full. Pay particular attention to authoring, JSON architecture, resources, and the phase 2 boundary. Record concrete dependency, schema, directive, and resource-origin decisions in [decisions](../DECISIONS.md).

## Deliverables

- A minimal Rust project with the CLI separated from internal rendering responsibilities, runnable focused checks, and CI for those checks.
- Typed theme/configuration structures, a versioned JSON Schema, and a bundled complete default theme covering all phase 1 fields, including later template and citation needs.
- Strict validation, token resolution, measurement semantics, partial-theme merging, and documented override precedence.
- Resource references that preserve document, theme, and bundled origins; initial source/property diagnostics.
- Documented front-matter and directive syntax sufficient for subsequent milestones. Keep the public API deferred.

## Completion criteria

A real CLI invocation loads valid document settings and a partial theme, resolves configuration, and diagnoses invalid inputs without claiming to render a PDF. Focused checks exercise object/array/null merge behavior, override order, tokens, units, version/field errors, and origins from another working directory.

Schema and typed configuration agree on accepted inputs. Check the default and at least one partial custom theme. Verify dependency and bundled-data license suitability before incorporating assets. Rendering remains explicitly unavailable until milestone 02.
