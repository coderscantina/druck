# 20: Writing partner

## Goal and prerequisites

The agent panel becomes an IDE-style writing partner, as in [the desktop briefing](../DESKTOP_BRIEFING.md#agent). Requires milestone 18. Local history labels come from milestone 21; until they land, the existing per-hunk revert stands in. See [the second](../mockups/02-agent-style-history.html) and [the third mockup board](../mockups/03-review-versions.html).

## Deliverables

1. **Off until enabled.** A setting, with setup that names an API key. No trace on screen when it is off, and no process spawned.
2. **`Cmd+J`.** With a selection, a prompt at the selection that works on it. Without one, a small chat card for book-wide work. Closing the card keeps the running turn and its results.
3. **Review in the text.** The CLI writes files with its normal tools. Changes show as tinted edits in the book's type with Accept, Reject, and Try again per passage, and Accept all and Reject all at the foot. This reuses the hunks that survive typing and reloads from the existing panel. Whole chapters arrive the same way, a new one included.
4. **Margin notes.** Advice beside its passage, stored in `.druck/` and anchored by quoted text, with Apply, Reply, and Dismiss. A note whose text is gone says so. The agent needs a small way to leave one, such as a local MCP tool. Choose it against the installed CLI's docs.
5. **Warnings.** Export and commit name unreviewed changes and do not block.

The docked conversation panel leaves, along with its rail tab.

## Completion criteria

With a real agent turn on a copy of the book: a selection edit, a passage refined across chapters, and a new chapter written and added to `druck.yaml`, each reviewed per passage, with no text lost while the author types during the turn. Rejecting everything restores the bytes from before the turn. With the agent off, no process and no chrome. Tool approvals still work, and a waiting approval is seen from the bare page.
