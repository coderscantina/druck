# 05: Mid-page column layouts

## Goal and prerequisites

Add one/two-column flow without losing page-composition guarantees. Requires milestone 04.

Read mid-page columns, footnotes, page composition, and directives in [the briefing](../BRIEFING.md). Keep ownership of coupled page/column/footnote logic explicit.

## Deliverables

- Explicit one/two-column sections with equal widths and a configurable gap.
- Reading-order flow and continuation of sections across pages.
- Final-column balancing and subsequent layouts below the tallest column.
- Explicit full-width blocks that balance prior columns and resume columns below; image/table integration follows in milestones 06/07.
- Shared full-width footnotes that participate in balancing and pagination.

## Completion criteria

Render and inspect one-to-two-to-one changes on one page, a section spanning pages, and a full-width text block followed by resumed columns. Include footnotes and keep-together constraints in the same document.

Check reading order, preserved text, column balance, the shared footnote area, and stable layout. A layout change must not force a new page when its content fits. Diagnose impossible combinations without clipping or excessive spacing.
