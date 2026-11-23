# 28: Beta polish

## Goal and prerequisites

Clear every known defect and add what a beta build needs to update itself, before the gate of milestone 22. Requires milestones 18 to 27 as built. Settled in [the beta polish decision](../DECISIONS.md#2026-11-23-beta-polish). The beta is for the owner only, on macOS.

The work runs in three waves. Each wave merges into `main` before the next starts.

## Wave 1: defects

Engine, on the CLI and the public crate:

- `DisplayDocTitle` is written for every PDF, not only PDF/UA. Line-end hyphens in tagged text are soft hyphens.
- In two-column sections, a margin note goes to the margin nearest its column. Single-column sections keep the outer margin.
- An unplaced figure never precedes a waiting placed one. A split callout's box keeps clear of the footnote area whatever `footnotes.gap` is.
- The bibliography heading opens a chapter as the index heading does. Index page numbers use the text color. The default palette's keyword blue and type teal are told apart.
- Title blocks keep leading slot space at the page top, and cover metadata takes its own top margin.
- Unquoted decimals in `meta` and `author` keep their source text (`1.50` stays `1.50`).
- A URL with no slash that is wider than its column breaks between characters instead of failing.
- `{pages}` and the other page placeholders work in the body: the settling loop carries the page count.
- `{build-date}` uses the local time zone, and `SOURCE_DATE_EPOCH` still wins. An optional `reading-speed` in `meta` replaces the language default for `{reading-time}`.
- The public crate: `Overrides::parse`, `Project::open`, and `DocumentText::read` return `Failure`. The layout exposes the contents depth and the theme's titles of made pages (contents, index, bibliography), additively, recorded in [the phase 2 API](../PHASE_2_API.md).
- Memory held by a long-lived process is measured on Linux; an allocator changes only if it measurably helps.

Desktop, the writing surface:

- `Cmd+Z` after an outside change (an agent or another app) undoes that change as one step.
- Index marks are hidden until the caret is on their line, also in table cells and captions.
- Shift-selecting across a hidden fence and deleting keeps the fence whole. Down on a code block's last line at the end of a file leaves the block.
- Table column widths in Write follow `widths`.
- A single file opens on its first text, not its front matter.
- The block menu and the lists filter as you type, and arrow keys move between starters.
- Section template text follows the book's language.
- The app shows the engine's contents depth and made pages' titles once the crate exposes them (wave 2).

Desktop, the agent and the typesetter:

- Consistency starts with one read-only turn over the whole book that collects the rules, then per-chapter turns apply them.
- A typesetter theme value is offered when it clears more warnings than it causes, with both counts and the caused pages on the entry. Other fixes keep the strict rule.
- An entry's pages repaint after another entry is accepted or rejected. "Clears n warnings" counts warnings a page break excuses.
- The typesetter's tool and a pass work on what is on screen: unsaved text is saved before a run starts, and the tool reads the session's text.
- Codex: a deny reason reaches it, a change to a file that is not open shows in the turn summary, and its approval card cuts a long line around the change. The app warns when the installed CLI is outside the tested version range.

Desktop, the shell:

- The German interface uses infinitives for commands and labels, and "du" only where the app speaks to the author.
- Spelling checks each paragraph in its own language. The words file stays `druck-words.txt`.
- Local history follows moves made in the app. A file that appears with the text of a vanished file's last version takes over its history.
- The settings card keeps Tab inside it. A clean quit leaves no recovery copy.
- The launch card scrolls when it outgrows the window. `passes/` files show in the Binder and open in the app.
- Publish shows a run once. The Proof panel is a few keys from the title bar. The selection toolbar steps aside for the palette.
- The two flaky tests, `source::tests::an_edit_resegments_a_ten_thousand_word_chapter_far_below_one_frame` and `spell::macos::tests::the_documents_language_decides_not_the_texts`, pass in a full parallel run.

## Wave 2: builds that update

- Panics and errors go to a log in the app folder. A Help item opens a prefilled GitHub issue with the log, and nothing leaves without a click.
- Sparkle 2 in the bundle, checking once a day and asking before it installs, never while typing. The appcast and the DMG are on the GitHub release, signed with an EdDSA key that never enters the repository.
- `release.yml` builds, signs, notarizes, and signs the appcast on a macOS runner from repository secrets. `CFBundleVersion` is a number that only rises; `CFBundleShortVersionString` stays `YY.M.D`.
- The icon stays the placeholder.

## Wave 3: gates

Milestone [10](10-release-acceptance.md) and milestone [22](22-writer-beta.md), with the owner's checks in one ordered script, and a final review of the whole build that covers regions and the pages in the Binder.

## Completion criteria

Every item above is built with focused tests, `cargo test`, clippy, and fmt are clean for both crates, and CI has run on Linux, Windows, and macOS. Visual changes are inspected on screen in dark and light, English and German. What only the owner can verify is listed in the script for wave 3.
