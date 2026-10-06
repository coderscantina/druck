# 02: First text-to-PDF rendering path

## Goal and prerequisites

Produce the first actual PDF through the CLI. Requires milestone 01.

Read the supported content, paragraph typography, PDF output, resources, and reproducibility sections of [the briefing](../BRIEFING.md), plus the current configuration decisions.

## Deliverables

- Source-aware CommonMark parsing for prose, headings, lists, quotations, inline formatting, and code.
- Bundled default fonts, shaped text metrics, kerning, ligatures, and internal positioned drawing content.
- A simple text flow across pages using resolved styles and page geometry.
- PDF output with embedded fonts, selectable/searchable text, and external links.
- Clear diagnostics for content whose rendering is not implemented yet. Preserve supported content rather than silently dropping nodes.

## Completion criteria

Render English and German sample Markdown through the CLI, inspect the pages, and verify text extraction and embedded fonts. Exercise relative font resources and rendering offline without relying on system fonts. Verify source locations for failures and explicit errors for unfit code lines.

Basic line wrapping and pagination may be temporary here. Record them as remaining milestone 03/04 criteria rather than completed typographic behavior. Capture initial runtime and memory measurements for representative document lengths; these are a baseline, not the final release guarantee.
