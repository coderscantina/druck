# 04: Scored pagination and footnotes

## Goal and prerequisites

Make page breaks account for constraints and footnote height. Requires milestone 03.

Read page composition, footnotes, diagnostics, and acceptance in [the briefing](../BRIEFING.md). Follow established metrics and source mapping decisions.

## Deliverables

- Scored page breaks, bounded vertical spacing, and short final pages.
- Heading retention, widow/orphan handling, explicit page breaks, and keep-together groups.
- Full-width page-bottom footnotes in reference order, with same-page starts and long-note continuation markers.
- Diagnostics for unsatisfiable constraints and content that cannot fit.

## Completion criteria

Render a document combining headings near boundaries, multi-paragraph groups, several footnotes, and a long continuing note. Verify preserved content, reference/note placement, absence of overlap, and repeatable breaks. Inspect the actual pages and record the result.

Footnote space must affect break selection. Include a case where adding a note changes the page break and a case requiring continuation. Keep the page composer ready to accept multiple column regions in milestone 05 without implementing the public phase 2 API.
