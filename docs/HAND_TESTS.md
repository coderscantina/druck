# Hand tests for the beta

Everything only a person can check before the beta is done, in an order that takes one sitting. It covers the owner's checklist in [progress](PROGRESS.md#owners-checklist), the open checks of milestones 25 and 26, the gate of [milestone 22](milestones/22-writer-beta.md), and what [milestone 28](milestones/28-beta-polish.md) could not drive by script. Note each result next to its box: what happened, and a capture when something looks wrong.

## Before you start

- Build the bundle: `python3 scripts/package-desktop.py`. Without the Sparkle key it has no updater, which is expected until step 12.
- Copy your book to a scratch folder and add a `druck.yaml` to the copy. Work only on the copy until step 10.
- Keep the Accessibility Inspector, the Japanese input method, and the German keyboard with dead keys at hand.

## 1. First launch from the Finder

- [ ] Open the bundle from the Finder, not from a terminal. The launch card shows the gallery and the agent offer once.
- [ ] Say yes to the offer. The Agent tab finds your CLI and its login.
- [ ] Open a Markdown file by dropping it on the Dock icon, and the project copy through Open.
- [ ] Make a new book from a starter through the system save panel.
- [ ] At 720 by 480 with the offer showing, scroll the launch card with the trackpad.

## 2. Input methods

Japanese, then German dead keys (`¨` + `a`, `´` + `e`). Composition shows in place and commits once, with nothing doubled or lost.

- [ ] The text, at a paragraph's start, middle, and end.
- [ ] A proof slip in Publish.
- [ ] The `Cmd+J` prompt, right after launch: type at once and the letters land in the prompt.
- [ ] The commit card (`Cmd+Shift+S`).
- [ ] A footnote popover.
- [ ] A table cell.
- [ ] A title page field in the Binder.
- [ ] A theme field and a chapter note.

## 3. A real mouse

- [ ] A click on a page in Publish goes to its place in the text.
- [ ] Drag a row in the Binder, a region's grip, a mark's grip, and the second column's grip.
- [ ] Click a table cell near the window's foot: it scrolls into view. Click its ⋯ handle.
- [ ] Click a chip, a figure, a margin note's More, and the Add a page sheets.
- [ ] Click the buttons of the conflict line, an approval card, the offer, and the typesetter's queue.
- [ ] Remove line on a mark, by mouse.
- [ ] Scroll the palette list and the Proof list during Fix the proof with the trackpad.

## 4. VoiceOver

- [ ] The sidebar, the rail, a panel, and the galley. The galley only reports the lines on screen: note how it reads.
- [ ] Arrow through the text, a table cell ("Row 1, column 2"), and a footnote popover ("Footnote" and its number).
- [ ] Open the block menu (`Ctrl+Return`) and arrow down. Is each row spoken? A checked row should say so, the Style row its value. Then the slash menu and a table's menu. Rows in floating menus may stay silent: this is the one known gap.
- [ ] On a margin note, `Ctrl+Tab` to Apply: the note's title and text are read.
- [ ] `Cmd+Y`, then Left and Right: each moment is spoken. `Cmd+Shift+S` says "Commit".
- [ ] Add a page from the Binder's +, then the arrows: the chosen sheet is spoken.

## 5. Menus and quitting

- [ ] The menu bar in English and German: the Insert menu, "Check for updates…" (only in a bundle with Sparkle), and "Report a problem…".
- [ ] "Report a problem…" opens the browser on an issue prefilled with the versions only, no log lines, and shows `druck.log` in the Finder. Send nothing.
- [ ] `Cmd+Q` right after an alert, with the real keyboard.

## 6. Dialogs

- [ ] Open, New book, the cover, adding a font, adding an image, deleting a shared theme, and a file dropped from the Finder.

## 7. The writing partner and the processes

These cost tokens. Use your own CLI settings.

- [ ] A chat turn with `defaultMode: auto`: it asks nothing, and the change shows only in the panel. Autosave feels calm while you type.
- [ ] `Cmd+Z` right after the agent's change takes the whole change back, and Redo brings it back. Type during the turn and check that nothing typed is lost (one run lost three letters once).
- [ ] Milestone 26: the consistency pass over the whole book. It starts with one read-only turn. Note the cost and the time.
- [ ] Milestone 27: Fix the proof on all of the book's warnings. Accept a theme value that shows both counts, then check that the queue repaints.
- [ ] One chat turn and one pass with Codex.

## 8. Single files

- [ ] Milestone 25: open a single Markdown file, change its title through Details by keys alone, and check the front matter.
- [ ] A single file opens on its first text.

## 9. Looks to judge

- [ ] The accent caret line on the page, the small footnote mark, chips, the grid over a table, "page of Table 1" on a page reference's chip, and a long theme field in the Source panel.
- [ ] In two columns a note from the inner column moves to the outer margin when the inner margin is too narrow. Keep that rule?
- [ ] The changed pages of `report`, `report-de`, and `images`, where pages before a tall figure now fill up.
- [ ] English underlined in a German book is gone: paragraphs are checked in their own language.
- [ ] The German text: infinitives on commands, "du" where the app speaks to you. The book offer says "Front Matter" for the YAML block: keep it?

## 10. A real chapter

- [ ] Write a real chapter of your book in the app, in the original folder, on the release build. Note anything that pulled you out of writing.

## 11. Milestone 10 by hand

- [ ] Look at every sample PDF in `samples/`, English and German.
- [ ] veraPDF, epubcheck, and Ace on the samples, with the commands in [release](RELEASE.md).
- [ ] On a Windows machine: `cargo test`, the sample renders, and `scripts/check-offline.sh` in Git Bash.

## 12. Signing and updates

- [ ] Create the EdDSA key with Sparkle's `generate_keys`, put its public half in `SPARKLE_PUBLIC_KEY` in `scripts/package-desktop.py`, and add the six repository secrets in [release](RELEASE.md).
- [ ] Decide whether `com.coderscantina.druck` is the final identifier.
- [ ] After two releases (a separate request each), install an update through "Check for updates…" and see Sparkle's windows in German.

## 13. Decisions left

- [ ] Migrate the original book to a `druck.yaml`?
- [ ] Are `API_VERSION` and a CalVer package version enough once an outside client exists?
- [ ] Should the Binder be a `Ctrl+Tab` stop of its own, or stay on `Cmd+Alt+2`?
- [ ] Ask Anthropic and OpenAI in writing about spawning their CLI with a subscription login.
