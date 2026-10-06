# 06: Images and captions

## Goal and prerequisites

Add local image rendering to established page and column flow. Requires milestone 05.

Read images/tables, columns, resources, theme architecture, and diagnostics in [the briefing](../BRIEFING.md).

## Deliverables

- Local PNG, JPEG, and SVG processing and rendering.
- Captions kept with images, document-order placement, and proportional downscaling.
- Column-width placement by default and explicit full-width placement.
- Document-relative and theme-relative image origins preserved through configuration resolution.

## Completion criteria

Inspect images with captions near page and column boundaries, oversized images, and full-width images between column regions. Include footnotes on the same pages. Verify proportional scaling, correct order, no overlaps, and repeatable placement.

Exercise missing, malformed, and unsupported image inputs with meaningful source/resource diagnostics. Render from a different working directory. Preserve local/offline loading and the explicit full-width requirement; automatic floats and text wrapping remain outside scope.
