# 17: Desktop 1.0

> Superseded on 2026-11-05 by [milestone 23](23-desktop-1-0.md), which keeps its deliverables under the new Style index.

## Goal and prerequisites

The complete theme editor and the other platforms. Requires milestone 16 and feedback from the beta.

## Deliverables

- A hand-designed panel for each remaining top-level theme section. Lists, tables, callouts, footnotes, margin notes, placement, captions, bibliography, contents, index, matter, chapters, scene breaks, typesetting, title block, title page with its anchored slot groups, page variants and bands, watermark, highlighting, labels, and the rest of the schema's 30 sections. Each schema change means UI work.
- Linux and Windows builds with their spell checker backends, packaging, and IME checks.
- Decisions from the beta recorded in [decisions](../DECISIONS.md).

## Completion criteria

Every schema property is reachable from a designed panel, checked by a test against the schema, and inspected on screen in dark and light, with the preview showing each change.
