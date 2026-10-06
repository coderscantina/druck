# Phase 1 milestones

## Why ten milestones

Use ten milestones to separate configuration, the first rendering path, paragraph composition, page composition, columns, images, tables, document structures, citations, and release acceptance. These are dependency and verification boundaries, not fixed token budgets or promises that each fits one conversation.

The three delivery stages in [the product briefing](BRIEFING.md) remain intact: milestones 01–03 implement core composition, 04–07 implement page and column constraints, and 08–10 complete documents and distribution. The public crate remains [phase 2](RUST_CRATE_API_PHASE_2.md).

## Sequence

1. [Foundation and theme contracts](milestones/01-foundation.md).
2. [First text-to-PDF rendering path](milestones/02-text-to-pdf.md).
3. [Optimized paragraph composition](milestones/03-paragraphs.md).
4. [Scored pagination and footnotes](milestones/04-pagination.md).
5. [Mid-page column layouts](milestones/05-columns.md).
6. [Images and captions](milestones/06-images.md).
7. [Multipage tables](milestones/07-tables.md).
8. [Document structures and templates](milestones/08-document-structures.md).
9. [Citations and bibliography](milestones/09-citations.md).
10. [Release acceptance and distribution](milestones/10-release-acceptance.md).

Each milestone depends on the preceding milestones unless its brief states otherwise. Work may be split into smaller sessions. Begin a later milestone only within the user's authorized scope and with its prerequisites satisfied.

## Session size and context

Start with AGENTS.md, progress, the current milestone brief, relevant decisions, and the related source and tests. Read the product briefing sections linked by the milestone. Load the full product contract for architecture and scope decisions.

Choose one observable behavior or a tightly coupled set per work slice. Reserve enough session capacity to verify it and write a useful handoff. If implementation breadth exceeds the current slice, preserve the remaining work in progress rather than silently reducing the milestone's scope.

Do not fill context just because capacity is available. Keep logs targeted, batch independent reads, and avoid repeated full-file output when a section or diff answers the question. Avoid model-specific token counts in this plan: the useful limit depends on the work and actual environment.

## Completion gate

A milestone is complete when all of its stated behavior is implemented, focused checks and its integration scenario pass, and required visual review is recorded. List unavailable checks explicitly. A milestone waiting on an acceptance check remains incomplete.

Add fixtures when their features arrive and extend them at later integration boundaries. Unit tests check algorithms and boundary behavior; rendered sample inspection checks appearance. Do not postpone all visual work to milestone 10.

Update [progress](PROGRESS.md) after work and record lasting choices in [decisions](DECISIONS.md). Keep model transcripts, generated logs, and scratch outputs out of these records. Milestone 10 prepares release artifacts; publishing requires a separate request.
