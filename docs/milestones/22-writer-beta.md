# 22: Writer beta

## Goal and prerequisites

The gate for the first public release, on macOS. Requires milestones 18 to 21 and 24 to 28, though a release may follow any of 24 to 27 if the owner says so. Since [the beta polish decision](../DECISIONS.md#2026-11-23-beta-polish) the first beta is for the owner only, and the open checks of 25 and 26 are part of this gate. Publishing needs a separate request.

## Deliverables

- The agent's setup documented with an API key for Claude and for Codex. A subscription login is documented only with the vendor's written answer in hand.
- Signed and notarized packaging, and the app's icon.
- Accessibility and keyboard pass over every new surface with AccessKit.
- Updated measurements, and the open items of [the owner's checklist](../PROGRESS.md#owners-checklist) that still apply.

## Completion criteria

The soak from milestone 16 repeated on a copy of the owner's real book: an hour of scripted edits with no lost text, typing under 16 ms median, from the key's handler to the end of the first paint that shows it, as the gate measures it since the [third review](../DECISIONS.md#2026-11-05-third-review-findings), pages within about 1 second of a pause, and agents and the app editing the same chapter without loss. Every surface inspected on screen in dark and light, English and German, against [the design document](../DESIGN.md). The input method test passes in the text, a slip, the `Cmd+J` prompt, the commit card, a footnote popover, and a table cell. The owner has written a real chapter in it. Anything only a person can verify is listed, not assumed.
