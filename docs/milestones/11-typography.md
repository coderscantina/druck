# 11: Installed fonts and custom styles

## Goal and prerequisites

Let themes use installed font families with real weights, and let authors apply theme-defined styles to blocks. Requires milestone 09. Independent of milestone 12.

Read the [business documents amendment](../BRIEFING.md#business-documents-amendment), resources and reproducibility in [the briefing](../BRIEFING.md), and the [amendment decisions](../DECISIONS.md#2026-10-07-business-documents-amendment).

## Deliverables

- Installed font lookup: a family name in `fonts` that is neither bundled nor in `font-files` is resolved among installed fonts at the CLI boundary, so `src/config/` and layout stay free of system access. A family in `font-files` wins over an installed one with the same name. Font collections (`.ttc`, `.otc`) load by face index.
- A missing installed family, weight, or style is an error naming what was searched for, and no PDF is written. A font whose OS/2 `fsType` restricts embedding is embedded with a warning.
- Numeric weights 100 to 900 in block styles, with `regular` and `bold` kept as aliases for 400 and 700. Font families map weights and italics to faces, both for installed and file fonts.
- Block style properties `tracking` (letter spacing in em) and `uppercase` (Unicode case mapping, so ß becomes SS).
- Named custom styles in the theme with `based-on` one existing style plus overrides. One level of lookup, unknown bases and cycles are errors.
- Attribute syntax `{.name}`: at the end of a heading or paragraph's last line, and on a line of its own directly before a list. An unknown style name is an error at its location. Headings keep `{#sec:x}` and may combine it with a class.
- Style properties `keep-with-next` (the block never ends a page or column without the next block), `bullets` on styles based on `list`, and `number-gap` on heading styles: a leading "N." typed by the author is set hanging with that gap.
- Schema, default theme, [themes](../THEMES.md), and [authoring](../AUTHORING.md) updated.

## Completion criteria

Unit tests cover face matching by weight and italic, a collection face, the missing-font and restricted-embedding diagnostics, weight aliases, `based-on` resolution and cycles, each attribute position and its errors, `keep-with-next` at a page end, list bullets from a style, and the hanging number. Installed lookup is tested against a fixture font directory, not the machine's fonts; one manual check with a macOS installed family is recorded.

Render and inspect a sample with an eyebrow paragraph kept with its heading, a ✓ list beside a • list, letter-spaced uppercase labels, Medium and Light weights, and step headings with hanging numbers. Existing samples render unchanged.
