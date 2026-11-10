# 27: Typesetter

## Goal and prerequisites

The agent clears typesetting warnings and the author reviews before and after pages. Requires milestones 19 and 26. Settled in [the agentic beta decision](../DECISIONS.md#2026-11-10-agentic-beta-direction).

## Deliverables

1. **A trial tool in the engine.** Given a candidate fix, lay out a scratch copy and return the warnings before and after, with pages. It never touches the project. It is exposed to the agent over MCP and is the only way the agent judges a fix.
2. **What a fix may be.** The directives that exist, such as a page break, keep together, and figure placement, and book-wide theme values, such as tracking or a hyphenation setting. No new per-paragraph directive: "no per-block overrides" holds. Where only a rewording helps, the agent leaves a margin note.
3. **"Fix the proof".** One command in the proof tab and the palette. The agent works through the list in the background and keeps a fix only if it clears a warning and causes no new one.
4. **The queue.** Each kept fix is an entry with before and after pages, Accept and Reject, and Accept all. Nothing lands in the manuscript or theme until accepted. A theme value is one entry with the count of warnings it clears, and follows the rule for common themes: copy to the project first.
5. **Without an agent** the proof list works as today, with no fixes offered.

## Completion criteria

On a copy of the owner's book, with both CLIs: a run that reduces the warning count, each accepted fix verified by a fresh layout, and Reject all leaving the project byte-identical. Typing in Write during a run loses nothing. The trial tool is covered by tests of a fix that helps, one that does not, and one that causes a new warning. The queue is inspected in dark and light, English and German. Tokens and time for the run are recorded.
