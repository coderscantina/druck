# 07: Multipage tables

## Goal and prerequisites

Add wrapped tables with correct continuation and column integration. Requires milestone 06.

Read tables, columns, paragraph composition, and diagnostics in [the briefing](../BRIEFING.md). Use the established text metrics and page composer.

## Deliverables

- Markdown table parsing and column-width allocation using shaped cell text.
- Wrapped cells, row-height calculation, and page breaks between rows.
- Repeated headers for tables continuing across pages.
- Tables in the current column and explicit full-width tables within column sections.
- An error for a row taller than the available page area.

## Completion criteria

Render and inspect a multipage table with varied cell lengths, repeated headers, and adjacent prose and footnotes. Include a full-width table interrupting two-column flow and verify that columns resume correctly.

Verify row and cell text preservation, no row splitting, no overlaps, deterministic pagination, and the oversized-row diagnostic. Merged cells remain outside scope. Resolve interactions through the shared composition boundary rather than adding a separate table pagination engine.
