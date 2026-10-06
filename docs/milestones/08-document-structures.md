# 08: Document structures and templates

## Goal and prerequisites

Render complete document structures from the resolved theme. Requires milestone 07.

Read document structures, structured templates, PDF navigation, reproducibility, and acceptance in [the briefing](../BRIEFING.md).

## Deliverables

- Title blocks, optional abstracts, separate title pages, and numbered headings.
- Ordered title slots and fixed header/footer slots with required/optional data behavior.
- Title, first-body, normal, odd, and even variants with documented fallback and page-parity rules.
- Table of contents, page numbers, running headers, labels, and heading/figure/table cross-references.
- Clickable internal links and heading bookmarks, with stable final numbering and page information.

## Completion criteria

Render default and custom-themed reports including separate title pages, a table of contents, mixed columns, figures, tables, and footnotes. Check variant selection, optional-slot spacing, required-slot errors, and final page references. Inspect the actual PDFs and navigation.

Include references whose generated content affects layout; final page numbers and repeated renders must agree. Record deterministic convergence behavior and diagnose failure to stabilize. Metadata values remain text rather than executable markup. Re-run this integration scenario after bibliography content is added in milestone 09.
