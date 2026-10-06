# 03: Optimized paragraph composition

## Goal and prerequisites

Replace temporary wrapping with paragraph-wide line-break optimization. Requires milestone 02.

Read paragraph composition, default presentation, and acceptance/performance in [the briefing](../BRIEFING.md). Inspect the shaping, metrics, and PDF path established in milestone 02.

## Deliverables

- Paragraph-wide break scoring using actual shaped widths, permitted spacing, and hyphenation.
- English and German language-aware hyphenation data and behavior.
- Justification and optical margin alignment with deterministic line output.
- Consistent handling of inline formatting, code, and language selection without glyph-width adjustments.

## Completion criteria

Use focused examples where paragraph-wide choices differ from greedy wrapping, as well as hyphenation and mixed-style boundaries. Inspect English and German pages at multiple text widths; check spacing, punctuation alignment, preserved text, and repeatability.

Benchmark 10-, 50-, and 100-page equivalent documents using the working typography prototype. Record hardware, inputs, runtime, memory, and proposed performance limits. Set the limits from evidence and revisit them as later features change workload. Do not equate good paragraph output with completed page composition.
