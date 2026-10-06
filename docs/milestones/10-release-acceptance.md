# 10: Release acceptance and distribution

## Goal and prerequisites

Prove the complete phase 1 contract and prepare the CLI for distribution. Requires milestones 01–09.

Read [the product briefing](../BRIEFING.md) in full and inspect unresolved acceptance work in [progress](../PROGRESS.md). Every phase 1 requirement must have evidence; identify omissions rather than narrowing scope.

## Deliverables

- Complete English/German sample corpus and recorded visual review, including mixed columns, full-width blocks, continuing footnotes/tables, citations, and custom themes.
- Automated overflow/content/reference/stability checks and PDF font/text/navigation verification.
- macOS, Linux, and Windows checks, identical-layout comparisons, and offline bundled-resource checks.
- Measured runtime/memory for 10-, 50-, and 100-page documents against evidence-based limits.
- CLI usage, theme-authoring documentation, license notices, CI coverage, and reproducible distribution procedures.

## Completion criteria

Run the full acceptance gates for the completed renderer, inspect the sample PDFs, and record exact commands, results, hardware, and platform evidence. Resolve failures in the responsible feature boundaries. An unavailable platform or missing visual review remains an explicit incomplete criterion.

Prepare release artifacts using established tooling without publishing, tagging, or pushing unless separately requested. Confirm phase 2 remains deferred and list the actual internal boundaries that its crate/API work will expose. Stop when acceptance passes; unrelated hardening remains follow-up work.
