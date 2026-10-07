# 13: Rich tables

## Goal and prerequisites

Add tables with block content in cells, column spans, and styled rows, as needed for cost tables in offers. Requires milestones 11 and 12.

Read the [business documents amendment](../BRIEFING.md#business-documents-amendment), tables in [the briefing](../BRIEFING.md), the [milestone 07 decisions](../DECISIONS.md#2026-10-07-milestone-07-multipage-tables), and the [amendment decisions](../DECISIONS.md#2026-10-07-business-documents-amendment).

## Deliverables

- A `::: table` directive holding a nested Markdown list: one item per row, one sub-item per cell, the first row is the header. Cells hold any block content, including paragraphs and lists. Pipe tables stay.
- Directive attributes `align` (one entry per column) and `widths` (`*` takes the remaining width, `auto` fits the content). Without them the existing column fitting applies.
- `{span=n}` on a cell spans columns. A row whose spans do not add up to the column count is an error at its location. Row spans stay outside scope.
- `{.name}` on rows and cells applies custom styles from milestone 11.
- Theme `tables.header-rule` and `tables.row-rule` with thickness and color, and a row style property `rule-below: none`.
- Title page and band slots may name custom styles.
- Captions, labels, repeated headers, rows that never split, and the oversized-row error work as for pipe tables. `wide` from milestone 12 applies.

## Completion criteria

Unit tests cover parsing rows, cells, and attributes, span errors, column widths and alignment, rules and their opt-out, and repeated headers for list tables.

Render and inspect a cost table that continues over three pages with group rows, detail lists in a lighter style, a note row, and totals rows without rules. Compare an offer theme with the owner's reference PDF on the owner's machine, since its font is not in the repository, and record the result.
