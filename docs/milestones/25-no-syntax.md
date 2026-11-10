# 25: No syntax to learn

## Goal and prerequisites

An author never has to know Markdown, directives, or `{.name}`. Everything is reachable from the slash menu, the block menu, and Turn into. Source view stays as the way out. Requires milestone 18 and the [regions](../DESIGN.md#regions) work. Settled in [the agentic beta decision](../DECISIONS.md#2026-11-10-agentic-beta-direction).

## Deliverables

A menu path and, where it has options, a designed card for each construct that is source view only today:

1. **Directive options:** a region's or standalone directive's options from its menu, as fields, not text.
2. **Custom styles:** applying and removing a theme's custom style on a block or a span, from the block menu and the selection toolbar, shown only as its effect.
3. **Index marks:** mark a selection for the index, with the entry and subentry in a small card, shown as a quiet mark.
4. **Front matter:** a single file's front matter through the book details that a project has.
5. **Code blocks:** insert, set the language from a list, and leave by keys.
6. **Link definitions:** reference links edit as inline links do; the definition stays where it is in the file.
7. **An audit:** a test that lists every construct the source layer knows and names its menu path, so a new construct without one fails.

Every edit stays a change to the text, and untouched blocks stay byte-identical.

## Completion criteria

Each construct is inserted, changed, and removed by keys alone on a demo chapter, with the file checked after each. The cards are inspected in dark and light, English and German, against [the design document](../DESIGN.md), which gains their rules. The audit test passes. Typing stays under 16 ms median.
