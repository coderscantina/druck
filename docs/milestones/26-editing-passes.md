# 26: Editing passes, with Codex

## Goal and prerequisites

Named, repeatable agent passes over a book, and a second agent CLI. Requires milestones 20 and 21. Settled in [the agentic beta decision](../DECISIONS.md#2026-11-10-agentic-beta-direction). Read the [second review's cost display decision](../DECISIONS.md#2026-11-05-second-review-findings) before designing progress.

## Deliverables

1. **Offered once.** First launch asks once whether to turn the agent on and says what it needs. Yes shows the agent only where a process applies. No keeps today's no-trace behaviour. This replaces "off until enabled in Settings" as the only way in.
2. **Pass files.** A pass is a Markdown file: a name, what it returns (changes or notes), and its instructions. Four ship built in. Customizing one copies it to `passes/<name>.md` in the project, and a project pass wins over a built-in of the same name. Authors add their own there.
3. **The four passes.** Copyedit and consistency return tinted changes. References and line edit return margin notes only. Copyedit never rephrases. Consistency keeps a style sheet file in the project that the next run reads.
4. **Running one.** Only when asked, from the palette, on the book, a chapter, or a selection. A local history label before each run. A quiet progress line, and results in the review that exists. Whole-book runs work chapter by chapter so one failure does not lose the rest.
5. **Codex.** One user-level setting chooses `claude` or `codex`. Both sit behind the agent module with a version check, and Druck's tools reach both over MCP, as `leave_note` does. Check the installed CLIs' docs for streaming, approvals, and MCP before building. Setup documents an API key for each; a subscription login is documented only with the vendor's written answer.

## Completion criteria

With real turns on a copy of the owner's book: each pass on a chapter with both CLIs, a whole-book consistency run that writes and then reads its style sheet, a customized pass winning over its built-in, and an author's own pass. Rejecting everything restores the bytes. No text is lost while typing during a run. Declining the first-launch offer leaves no process and no chrome. Approvals work with both CLIs. The offer, the palette rows, and the progress line are inspected in dark and light, English and German.
