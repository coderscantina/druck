# Desktop design

Read this before UI work. When the owner settles a visual rule, write it here, in the section it belongs to.

## Direction

Writing first: while the author writes, the text is nearly all there is. Publish is a separate step with its own view. The owner settled this on 2026-11-05, choosing from [the mockups](mockups/), and [the decision](DECISIONS.md#2026-11-05-writing-first-direction) records what lost and why. Two things on the boards were later changed: the command palette is on `Cmd+P`, not `Cmd+K`, and the commit card is on `Cmd+Shift+S`, not `Cmd+S`. The settings window has no per-book switch.

The look follows the owner's reference image: neutral greys with no colour cast, thin dividers, compact rows, softly filled fields, and a right rail whose active tab is raised and joins its panel. Do not copy it: Druck sets its own tokens, icon set, and details. Dark and light both ship.

Write shows the theme's fonts, sizes, weights, spacing, and hierarchy on the app's surface, with colours following the appearance. Publish's pages show the theme exactly, in its own colours. App chrome never changes what a page shows.

The accent is the author's choice, oxide red by default. It colours only the caret, selection, marks, and the action that moves on. Nothing else is tinted.

Colors, spacing, and type live in one table in the desktop crate, `desktop/src/tokens.rs`. Views never hardcode a value.

Every string is in English and German. A German label must fit the same width as its English one, so prefer the short word.

## The window

The title bar holds only the mode switch at its centre, Write and Publish, and in Publish the Export button at its right, in the accent color. Everything else is on `Cmd+Shift+P`.

The window has places to mount a view without touching the shell, listed in `desktop/src/app.rs`. Rail tabs take a panel with `InspectorView::mount`.

The launch screen is one small card of rows on the canvas color: open, then the starters.

## Write

- By default, on a bare page, only the text and the mode switch show. No sheet, no gutter at rest, no footer.
- Chapter ticks stand at the left edge, one short faint line per chapter, the current one longer and in full ink. Hovering opens the contents as a card over the edge. A tick never marks a page problem.
- The word count is faint at the bottom right, with a thin space between thousands. It hides while typing. The saving state shows only when something is wrong.
- `Cmd+\` shows the Binder, described below. Settings shows the same value.
- Lines wrap at the book's measure by default, centred in the window. There is no justification and no hyphenation in Write, on purpose: these lines are for writing, and Publish shows the real ones.
- Chapter notes and agent notes stand in the right margin, with a thin accent line at their left, and only when there are any. The text stays where it is unless the window is too narrow to hold both.
- Zoom scales the text and keeps the characters per line. A faint value shows for a moment.

## Publish

- Spreads in the centre on the canvas color, then a panel and the rail on the right. The status line holds the page range, how many problems remain in the accent color, and at its right the zoom: minus, the value, plus, and Fit.
- The first page is a recto and stands alone right of the gutter. After it, pages pair up as a printed book's do, with a 2 px gutter.
- The rail's tabs are book details, style, fonts, and images at the top, and agent, proof, and export at the foot. A tab shows once something is mounted on it.
- One zoom scale: contact sheets below fit, then fit, which shows a whole spread, then larger. Zooming out past fit becomes a contact sheet of every page, with a dot on a page that has a problem. A click zooms into its spread. Below fit there are four fixed sizes, so a sheet looks like one whatever the window.
- A proof slip is a card under its paragraph on the panel color, with the paragraph's written text in the theme's font at 14 px, the two directive buttons, and at its foot the paragraph's style as a link. It follows the paragraph, and the view follows it to another page. Escape closes it, and so does a click on no text. The marked paragraph has a thin accent outline.
- A block in a list, a quotation, or a group kept together shows no directive buttons, since a directive line cannot stand there.
- Pages mark a problem with the same outline and a faint tint, and the one being looked at with a stronger tint. A warning about a whole page marks the page's text. Only proof marks appear on pages, and only in this view.
- The accent stays on the action that moves on and on marks. A chosen segment and a chosen item are the raised grey with full ink, never the accent.

## Settings

A separate window after Zed's: categories on the left, then a search field, then rows. A row is a label with one quiet line of description at the left and its control at the right, with thin dividers between rows. The left list ends in a link to `settings.json`. A choice applies at once. Toggles are small squared switches, not pills. The accent is six square swatches and a hex field.

## Hierarchy

Section labels, field labels, and rows keep the mockups' hierarchy: a small section label, then quiet field labels, then values. Inspector panels start with the small section label on a divider, then quiet field labels above their fields. A detail view starts with a quiet back link before its title.

## Binder

The Binder is the old sidebar, 216 px, on `Cmd+\`.

- From the top: the identity block on the raised color, then flat lists separated by thin dividers. Rows are 30 px.
- A row is a lead column, a truncated title, and a slot for its dot at the end. The lead is the chapter's number, or an icon where it has none: T for text with no heading, a list for contents, an open book for an unnumbered heading, a page for a plain file.
- Titles come from the text: the first heading, without its Markdown. A file with no heading shows its name without the ordering prefix. There is no section type behind any of this.
- The selected row uses the raised color and full text color. Other rows use the quiet text color, numbers and icons the faint one.
- With the keys in the Binder, the selected row has a 2 px accent bar at its left edge. Up and down move, Home and End jump, Enter or Escape return to the text.
- A row's edit button shows on hover. A file not in the book has one visible action, its plus. Notes and research fold at their labels and start open.
- A dragged row drops above the row under it, shown by a 2 px accent line at that row's top.
- The bar at the sidebar's foot holds the new section button at its right end.
- A dot means a broken reference or citation, or an open placeholder, nothing else. Page problems never show here. It is derived from the diagnostics list and never stored. The dots match the proof tab: filled accent for an error, hollow for an open placeholder.

## Dialogs and cards

- A dialog is a small card over the window, near the sidebar it came from, as in the mockups, with its heading and a close button. Templates list as an accent icon, a name, and one quiet line of description, under quiet group labels.
- The question about a common theme is a card near the sidebar, like the section dialog. The recommended choice, a copy in the project, has the accent border.
- The search panel and the find bar are cards over the text's corner, not modal: the card draws no backdrop, so the text stays usable while it is open.

## The text

- The block gutter is empty at rest and while typing. Its type icons show when the pointer enters the left margin, the hovered block's in full ink, the others in the quiet color. Nothing else moves when it appears.
- A heading's icon is three short lines and its level. A dragged block drops above the block under the pointer, shown by a 2 px accent line, the same rule as a dragged sidebar row.
- A hidden comment takes no room, and a small speech mark stands right of the type icon on the row it was on. For a comment block it stands beside the next block that shows. Clicking it shows the comment in the flow, in the quiet color.
- A selection shows its text clean, with no markers revealed. Blank gaps between blocks show no selection.
- The scrollbar is a 6 px thumb in the pane's right edge, faint, and stronger when the pointer is over it.
- The source view uses one monospace on the same surface with no block backgrounds. Markers, comments, directives, and front matter are in the quiet color, headings bold, and code in the link color.
- Passages an agent changed show with the removed text struck through on a `removed` tint and the new text on an `added` tint, both in the book's type and at 22 percent, with small Accept, Reject, and Try again buttons below, at the text's left. The removed words come first and take a space of their own. When there are any, the foot of the window shows how many in this chapter and elsewhere, with Next if any are elsewhere, then Accept all and Reject all, which act on all of them. Accept has accent text. The tints stay while typing, and nothing moves on a pause.
- A misspelled word has a 1 px line under it in the accent color at 60 percent, in the text and the source view. It takes no room and leaves as soon as its block is edited, until the next pause. Its menu uses the slash menu's rows: suggestions with a text icon, then a plus icon and the row that adds the word to the project's words. The word stays selected behind the menu.

## Floats

- The floating toolbar is one row on the panel color with a thin border and a shadow: marks, a thin separator, then quotation, lists, and link. A mark that is on inverts its button, text color behind panel color, as in the mockup. A mixed mark shows its icon in the accent color. There is no `Tt` button.
- It sits below the selection, or above it when there is no room below, and stays over the text up and down and inside the window sideways. Only a selection taller than the window gets covered, at its top.
- A command that selects a placeholder opens no toolbar, and Escape hides it until the selection changes.
- Lists that float, the slash menu and a block's convert menu, start at the caret's or icon's left edge, not centered on it. Rows are 28 px on the panel color, with a faint icon that turns accent on the chosen row. They show ten rows and scroll.

## Rich blocks

- A footnote's reference shows as a small mark: its number in the book, in superscript digits, in the link color, with no box. A citation or cross-reference is a chip: the engine's text in the link color on a 12 percent tint of it, with a 3 px radius. A key, label, or note the book cannot resolve is the same chip in the error color, showing its source. In Write the colors come from the appearance, so a chip stays legible in dark and light.
- A chip in brackets is one step for the caret and one key for Backspace. A short form, `@key` or `@sec:x`, shows its source while the caret is at its end, since it is being typed. A caret brought inside a chip shows its source.
- A note's text has no block of its own while a reference names it. It opens in a popover at its mark. A definition with no reference shows as a raw block, in the small size.
- Floats that have fields are cards on the panel color: a section label or a quiet title, then fields, then small 22 px buttons as in the agent panel. The choice that lets the work go on has accent text. Nothing is filled.
- A table at rest shows as a grid in the text: its caption above it, a rule above and below the header and below the last row, thin rules between rows, the header bold, and the columns' alignment. A click opens the editing grid over it at the same place, a card with its tools above, then its cells, then caption, label, and placement. One field moves between the cells, and the others show their Markdown as written.
- A figure at rest shows centered, at most as wide as the column and three quarters of that tall, with its caption below. A click opens its card below the image. A missing file is a tinted box with the error color's message.
- What is refused says why in the accent color, inside the float that asked, in the engine's words. A deleted reference offers to remove its note's text in a small card at the caret, which takes no focus and leaves with the next edit.

### Selection

The controls for a selected block's named style live in Publish's Style tab, reached from the proof slip's link. Text alignment is five segments: the theme's own, left, center, right, justify. Choosing one picks or makes a named style `base-align` and writes `{.name}`, and Reset takes it off. Below it the style list shows the theme's own first, then each custom style the block's kind may use as `.name`, then New style. There are no per-block settings.

### Images

A row is a 44 px thumbnail on the raised color, the path as the document writes it, and one quiet line with the size and how often the book uses it. A file the text names that is not there shows the images icon and "File missing" in the accent color. A click goes to the first use. Insert image stands at the panel's foot.

## Keyboard

Only a keyboard's focus shows: a 2 px accent ring inside the control's edge, drawn as an inset shadow over any border, so nothing moves and nothing fills. Tab and Shift+Tab walk the controls, and Enter or Space click. In the text, Tab stays in the text, and Control+Tab leaves it. Rows of the contents list stay out of the tab order, since arrow keys walk them: the Binder holds one stop.

The keys: `Cmd+1` and `Cmd+2` for the modes, `Cmd+\` for the Binder, `Cmd+P` for chapters, `Cmd+Shift+P` for commands, `Cmd+J` for the agent, `Cmd+S` to write the pending save, `Cmd+Shift+S` for the commit card, `Cmd+Y` for the timeline, and `Ctrl+Shift+G` for the Git panel. `Cmd+K` stays on Link and `Cmd+Shift+G` on Find Previous.

## Saving state and conflicts

- The saving state shows only when something is wrong or a note is due: one faint line at the bottom left. It does not move the text or take focus.
- A note, such as a merge that happened, uses the quiet text color and leaves after 6 seconds. A problem or a conflict uses the accent color and stays until it is over.
- A conflict never opens a dialog. It is a line at the foot of the window with its choices as small raised buttons, and the text keeps the focus and keeps taking keys. A modal prompt would catch keys typed while it appeared, and agents can cause many of these. The keys are Ctrl+Cmd+M, T, and B.

## Rail panels

The panels of Publish's rail. See [Publish](#publish) for the tabs.

### Theme panels

- The Style tab is the theme picker, a search field, and a list of the theme's 30 sections under quiet group labels, with a section's main value at the end of its row when it has one. A section opens as its own view with a quiet back link. Tokens and the theme's source are entries in the same list. A section with no designed panel yet opens as JSON in the same place, and one with a panel has a quiet JSON link at the bar's end. The Fonts row goes to the Fonts tab.
- A field is a label row, then its control. The label row has the label at the start, and at the end what the value comes to when it is a token (quiet), a small `▾ $` link when the field takes tokens, and an accent Reset link only when the theme itself sets the property. Values the theme inherits show in the faint text color, its own values in the full one.
- A token list opens inline below its field, never as a floating menu, so it cannot be clipped by the narrow panel. Colors have a swatch left of the field.
- The engine's refusal shows under the field in the accent color. A field the engine refused keeps what was typed.
- The picker's menu hangs from its button, lists the project themes, then the common ones, and keeps a `⋯` on each common row for rename, duplicate, and delete. Deleting asks first, in the system's dialog.

### Proof

The list groups by chapter in book order, with what belongs to no chapter, such as settings and themes, first under the project's name. A row is a dot, a quiet line with the kind and the line number, then the message in up to two lines. A click goes to the problem on the pages and `Cmd+]` and `Cmd+[` walk the list. The row being looked at is raised, and Write opens at that place next.

### Export

The panel lists each named export as a collapsed card: name, then one quiet line of its output and switches, then the run button. A card opens to its theme, output, and bleed fields and its three switches. Progress is three segments, one per stage, with the stage's name above and Cancel beside it.

### Agent

- It has no chrome until enabled: no rail tab, menu item, palette command, or key row. `Cmd+J` with a selection opens a one-line prompt card, 420 px, under the selection, which stays tinted, with the Enter sign at its right. Without one it opens a chat card, 300 px and at most 520 px high, at the bottom right, over the text. Closing it keeps the running turn, and the foot then says it is working, with Stop. In Publish, it is also a rail tab, and `Cmd+J` goes there.
- A waiting tool approval opens the chat card, and an accent dot on the Write switch marks it when the author is elsewhere.
- Margin notes have a thin accent line at their left, a title in full ink, one quiet line, a suggestion in the theme's font if there is one, then small buttons. Their passage has a thin accent line under it. They stand beside their passage, on the surface's colour over a chapter's own long notes. A note whose text is gone stands at the top of the margin, says so, and offers only Dismiss.
- Replies show as plain text. The author's prompts sit on the raised color. A tool call is one quiet line with its name and target, collapsed until clicked, with a word at its end only when it waits, was denied, or failed.
- Diffs in approvals use Menlo at the small size, one row per line with the line number, a sign, and the text. Added and removed rows are tinted at 14 percent in the `added` and `removed` colors.
- Buttons are 22 px high with a thin divider border and quiet text. The choice that lets the work go on has accent text. Nothing is filled and nothing is a pill. Chips are 22 px with a 4 px radius on the raised color, and end in a close icon.
- The setup text names an API key as the way to set Claude Code up and says what the login line reports. It never offers a login of its own.

## Versions

- The commit card is a card at the top of the text with no backdrop: a section label, a message field, the changed chapters with word counts, then the buttons. Commit has accent text. It names unreviewed agent changes in the accent color. Escape closes it and returns the keys.
- The timeline is a strip at the foot of the window, hidden until asked for, with the current branch at its left and the sync state at its right. Commits are tall marks in full ink, local history small faint ticks, and the position an accent mark. Above it the chapter shows at that point, with word differences on `added` and `removed` tints.
- The Git panel has the rail panels' form: section label, quiet group labels, a branch picker, the changes, a message, then history as rows with a quiet date. It uses Git's words. It takes 296 px beside the text, which re-centres. The branch list opens inline below the picker, never as a floating menu, as token lists do.
- The timeline covers the window below the title bar. The marks stand evenly along the strip, oldest at the left and now at the right, since a time scale would crush a day's ticks against years of commits. The line above the text names the chapter, the moment, and what the mark is, with Restore passage and Restore chapter at its right end. Its keys: left and right walk the marks, Option with them walks the commits, Home and End go to the ends, up and down choose a passage, Enter restores the passage, Command+Enter the chapter, and Escape closes.
- A word difference is against the text as it is now: what that version held and now is gone is struck through on the `removed` tint, and what has been added since is on the `added` tint. The chosen passage has a thin accent line under it.
- The first commit in a folder with no repo asks once, in the card. From then on the card says so in one quiet line, with the same button. A folder in iCloud or Dropbox has the warning in the accent color in the card and the panel, and a new project there says it once at the window's foot.
