# 16: Desktop public beta

## Goal and prerequisites

The [desktop app](../DESKTOP_BRIEFING.md) on the public crate, ready for a first public beta on macOS. Requires the phase 2 crate. Milestone 14's owner test (IME) stays open separately and must pass before a beta ships.

Everything here uses `druck`'s public surface, plus the unstable `editor` feature where the crate records a reason. No second layout path. Text is the truth.

## Slices, in order

1. **Project session.** Open a project folder or single file. A document session owns chapters, buffers, and the preview's joined text. The sidebar shows the identity, contents, an "Not in book" group, notes, and research. One chapter in the galley at a time, and a single file is split in view at level 1 headings. Reorder by drag, written to `druck.yaml`. New section from templates, and starter projects for book, report, and offer. The details panel writes `druck.yaml`, or front matter for a single file. Click sync across chapters.

The rest run in parallel after slice 1:

2. **Persistence.** Atomic autosave, a folder watcher, reload of external changes, three-way merge that keeps undo, and a conflict prompt when edits overlap.
3. **Editor commands.** Floating toolbar, slash menu, block gutter with convert and drag, source view per chapter, and chapter notes for comments.
4. **Theme core.** Fonts, tokens, and page geometry panels hand-designed, a JSON view for all other sections, a theme picker, common theme create, read, update, delete, forking into a project, and the bundled specimen with a jump to a block using the style.
5. **Shell.** Diagnostics panel, sidebar dots, export panel for named exports, project search, word count, the string table in English and German, and macOS menus.
6. **Agent panel.** Streamed conversation, tool approvals, a per-turn diff in the galley with per-hunk revert, a resumable session per project, and the current context passed along.

Then:

7. **Rich blocks.** Footnote popover, citation and reference pickers with chips, table grid, and image insert with assets copied into the project.
8. **Beta gate.** Print-composer line toggle, system spell checker behind its seam, accessibility pass with AccessKit, signed and notarized macOS packaging, updated measurements, and a review of every panel in dark and light against [the design document](../DESIGN.md).

## Completion criteria

The owner's real book, as a copy, is written in for an hour's equivalent of scripted edits with no lost text, flat live heap, typing under 16 ms median, and pages within about 1 second of a pause. Agents and the app edit the same chapter without loss. Every panel is inspected on screen. Anything only a person can verify is listed, not assumed.

Publishing a beta needs a separate request. So does confirmation from Anthropic on subscription logins, which must be in hand before a public build includes the agent panel with subscription login documented.
