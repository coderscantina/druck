# Working on Kyber

## Start and resume

Read [progress](docs/PROGRESS.md), then the brief for the milestone relevant to the requested work. Use [the milestone index](docs/PHASE_1_MILESTONES.md) to find it. Load related source, tests, configuration, and the product briefing sections named by that milestone before changing code.

[The phase 1 briefing](docs/BRIEFING.md) is the product contract. Read it in full when establishing architecture or changing scope. Read [the phase 2 contract](docs/RUST_CRATE_API_PHASE_2.md) only when working on the public crate or checking a design choice that could obstruct it. Phase 2 is deferred.

Read [decisions](docs/DECISIONS.md) when choosing dependencies, configuration syntax, interfaces, or layout behavior. Record concrete choices there when made, with their reason and consequences. Source and manifests remain authoritative for installed dependencies and runnable commands.

## Work boundaries

Complete the user's authorized work. A milestone index is a sequence, not permission to implement every milestone. Questions and requests for assessment are read-only. Keep unrelated findings as follow-ups.

Prefer small modules with clear responsibilities and typed boundaries. Keep CLI input/output separate from rendering. Expose the supported public crate in phase 2; keep phase 1 interfaces internal. Choose dependencies against actual requirements and check the installed version's documentation or source before using uncertain features.

Use targeted searches and edits. Read the current files before making claims about them. Add focused tests for stated behavior, especially system boundaries and layout interactions. Keep temporary verification artifacts outside the committed source and remove scratch files before finishing.

## Context and delegation

Work on a coherent slice with observable completion criteria. Load reference material when its branch is relevant rather than copying whole briefings into prompts or progress notes. Preserve unresolved work before a context handoff.

Use one agent for a focused task. Delegate independent work only when breadth warrants it; give each agent explicit file ownership, inputs, acceptance criteria, and a requested conclusion. Keep coupled paragraph, page, and footnote changes under one owner unless their interface is already established.

## Verification and handoff

Run checks that exercise the changed behavior. For visual changes, inspect the rendered pages. Use targeted checks during development and the milestone's integration gate at completion. Record the exact command, result, and any unavailable platform or review in progress. Mark a milestone complete only when its criteria are met; passing unit tests alone does not establish visual acceptance.

Update progress after meaningful work with completed changes, next action, blockers, and follow-ups. Keep the resume note short and replace stale notes. Link decisions and relevant files instead of duplicating them.

Commit, push, publish, delete data, and modify files outside this repository only when authorized. Preserve unfamiliar work. Write plain prose, short paragraphs, sentence case headings, and no em dashes.
