# 24: Gallery of finished looks

## Goal and prerequisites

A new author starts from a rendered book, paper, or thesis, not from theme JSON. Requires milestones 18 to 21. Settled in [the agentic beta decision](../DECISIONS.md#2026-11-10-agentic-beta-direction).

## Deliverables

1. **Five starters:** novel, non-fiction book, single-column article, two-column paper, and thesis. Each is a normal theme plus a starter project with sample text that shows its elements. They use what the engine has today, so no math and the two built-in citation styles.
2. **The gallery on the launch card and New Book:** each starter shown as real pages painted by the engine from its sample, never a stored picture. Pick one, type the title, and the caret stands after the title in Write.
3. **Starters replace the launch card's document kinds,** which covers milestone 22's "starters reduced to book, paper, and article". New section templates follow the starter.
4. **A project made from a starter is ordinary:** `druck.yaml`, `manuscript/`, and the theme by name. Nothing records which starter it came from.

## Completion criteria

Each starter renders with no errors through `druck check` and is inspected as pages. The gallery is inspected in dark and light, English and German, and is usable by keys alone. A project made from each starter opens, renders in Publish, and exports. The thesis has front matter, a contents page, and a bibliography; the paper sets its abstract full width above two columns.
