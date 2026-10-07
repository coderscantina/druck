# 12: Page geometry, covers, and bands

## Goal and prerequisites

Let a theme set non-mirrored margins, a prose width narrower than the frame, and covers and footers made of anchored slot groups filled with free document metadata. Requires milestone 09. Independent of milestone 11.

Read the [business documents amendment](../BRIEFING.md#business-documents-amendment), structured templates and page composition in [the briefing](../BRIEFING.md), and the [amendment decisions](../DECISIONS.md#2026-10-07-business-documents-amendment).

## Deliverables

- `margins.mirror`, default `true`. With `false`, inner is the left margin on every page, for the body, bands, and title page.
- `page.text-width`: the width of prose, measured from the inner edge of the frame, defaulting to the frame width. A theme list `wide` names block kinds that use the full frame width; at first `table`, `figure`, and `code-block`. Headings, paragraphs, lists, quotations, footnotes, and column sections use the prose width. Bands always span the frame.
- Anchored slot groups for the title page and for headers and footers. A group has an anchor on the margin frame (top, middle, or bottom by left or right), an offset, a width, an alignment, and an ordered list of slots stacked as title slots are today. Slots may span several lines. The current left, center, and right band slots become groups in the default theme, and existing samples render unchanged.
- A `meta` map in front matter with string or list-of-lines values, inserted with `{meta.key}`. A list inserts one line per entry. Other unknown front matter keys stay errors. A missing value omits the slot, or is an error for a required slot.
- A `{pages}` placeholder with the total page count.
- Schema, default theme, [themes](../THEMES.md), and [authoring](../AUTHORING.md) updated.

## Completion criteria

Unit tests cover mirroring on and off, prose and wide block widths, each anchor position and offset, multi-line slots and lists from `meta`, missing and required meta values, `{pages}`, and the existing band and title page behavior expressed as groups.

Render and inspect a sample with a cover holding a metadata block at the top right and a title halfway down on the left, a three-column multi-line footer with "Page 1|N" on the right, prose at a narrow width beside full-width tables, and non-mirrored margins over several pages. Existing samples render unchanged.
