# Desktop design

Read this before UI work. When the owner settles a visual rule, write it here, in the section it belongs to.

## Direction

Writing first: while the author writes, the text is nearly all there is. Publish is a separate step with its own view. The owner settled this on 2026-11-05, choosing from [the mockups](mockups/), and [the decision](DECISIONS.md#2026-11-05-writing-first-direction) records what lost and why. Two things on the boards were later changed: the command palette is on `Cmd+P`, not `Cmd+K`, and the commit card is on `Cmd+Shift+S`, not `Cmd+S`. The settings window has no per-book switch.

The look follows the owner's reference image: neutral greys with no colour cast, thin dividers, compact rows, softly filled fields, and a right rail whose active tab is raised and joins its panel. Do not copy it: Druck sets its own tokens, icon set, and details. Dark and light both ship.

Write shows the theme's fonts, sizes, weights, spacing, and hierarchy on the app's surface, with colours following the appearance. Publish's pages show the theme exactly, in its own colours. App chrome never changes what a page shows.

The accent is the author's choice, oxide red by default. It colours only the caret, selection, marks, and the action that moves on, and it says what is wrong: errors, open conflicts, and open placeholders. Nothing else is tinted. Marks are proof outlines on a page for an error or the slip's paragraph, a drop line, the position on the timeline, a misspelled word, and a dot that asks for attention. The keyboard's focus ring and the bar on a row that has the keys are marks too. A link such as Reset, a `$`, a `⋯`, a chosen item, a switch that is on, a focused field, a progress bar, and a list's icon are not: they use the greys.

Colors, spacing, and type live in one table in the desktop crate, `desktop/src/tokens.rs`. Views never hardcode a value.

Every string is in English and German. A German label must fit the same width as its English one, so prefer the short word.

## The window

The window is at least 720 by 480, and every view works at that size in both languages.

The title bar holds only the mode switch at its centre, Write and Publish, and in Publish the Export button at its right, in the accent color. Everything else is on `Cmd+Shift+P`.

The window has places to mount a view without touching the shell, listed in `desktop/src/app.rs`. Rail tabs take a panel with `InspectorView::mount`.

The launch screen is one small card of rows on the canvas color: open, then the starters, book, paper, and article, with quiet icons. It holds the keys, so Tab walks its rows. A folder with Markdown and no `druck.yaml` puts a row first, "Make this a project", in the accent and holding the keys with its focus ring showing, under one quiet line that names the folder, never its whole path. Its own quiet line says what it will do before anything happens, and names a path the book asks for that cannot be found. Once it has run, the window's foot says what was written. A starter asks where to go, in Documents, with a name in the app's language, is written in that language, and opens on its first heading with the caret after the title.

The window's title is the book's title, or the file's or folder's name while it has none.

## Write

- By default, on a bare page, only the text and the mode switch show. No sheet, no gutter at rest, no footer.
- Chapter ticks stand at the left edge, one short faint line per chapter, the current one longer and in full ink. Hovering opens the contents as a card over the edge, as wide as its longest title between 240 and 360 px. A tick never marks a page problem.
- The word count is faint at the bottom right, with a thin space between thousands. It hides while typing, and when the text or its margin would reach it. The saving state shows only when something is wrong.
- `Cmd+\` shows the Binder, described below. Settings shows the same value.
- Lines wrap at the book's measure by default, centred in the window. When panels take room the text narrows to what is left, down to `TEXT_MIN_WIDTH`, about 40 characters, and below that the Binder gives way while it stays on in the settings. No line is cut or covered, in Write or by a panel at the right. There is no justification and no hyphenation in Write, on purpose: these lines are for writing, and Publish shows the real ones.
- A chapter's opening comments are one line above its first text, in the flow, with a small note icon, in the faint colour, and nothing in the margin. The line is the note's first with something to say: a blank line, or one that only repeats the chapter's title such as `Kapitel 3: Warum 404 die richtige Antwort ist`, does not count. A click unfolds the whole note there, and the title moves down for it. The folded line is a stop of Control+Tab: taking the keys unfolds it and hands them to the first note. Escape folds it and gives the keys back to the text, and so do a click on the icon and leaving it. While the caret shows the note raw in the flow, no folded line stands over it. Command+Up lands on the chapter's title, never inside the folded note, and Command+Down stops before a closing comment, so a note opens only when the author goes to it. While the text's selection is inside the note, as a find leaves it, the note stays unfolded and shows that selection, so nothing changes out of sight. This is the N3 variant.
- A later comment stands in the right margin beside its passage, up to six lines in the faint colour, fading out, with a thin line beside it in the divider colour and a quiet More that opens the rest in place on a click or focus. This is the N1 variant. Agent notes stand in the same margin, with a thin accent line. The margin is one flow in reading order, so nothing is painted over anything else. The margin shows only when there is something for it, and the text stays where it is unless the window is too narrow to hold both. The text keeps its measure first: in a window narrower than measure plus margin, comments have only their gutter marker, and an agent's note only the line under its passage.
- Zoom scales the text and keeps the characters per line. A faint value shows for a moment. Write starts at 125 percent, about 18 px body text, until the author picks another zoom, and `Cmd+0` returns there.
- When the pages cannot be set, Write keeps the theme's type, read from its fonts without a layout.

## Publish

- Spreads in the centre on a surface of their own, `pages_canvas`, darker than a white page in the light appearance and the canvas color in the dark, then a panel and the rail on the right. The status line, on the canvas color like the title bar, holds the page range by the pages' places in the file, "Pages 14–15 of 203", with the printed numbers beside it in brackets when they differ, such as "(iii–iv)", how many errors and open placeholders remain in the accent color, how many typesetting warnings there are in the quiet colour with the settings' small switch and the word Marks beside them, then the window's foot line, and at its right the zoom: minus, the value, plus, and Fit. A narrow line drops the printed numbers first, then the word "warnings", then the switch's label, and never cuts a number or the zoom.
- A document with no pages shows, where they would stand, "No pages", every message in full, in the app's language with its file and line from the book's folder, and small buttons: Show in text in the accent, which goes to the first message's place in the text, when one has a place, then Book details and Proof. The status line then leaves the message out.
- A missing file does not take the pages away. A missing image stands on its page as a dashed box of a figure's size with its path, in a quiet grey, a missing bibliography leaves each citation as its keys, `[?key]`, and a missing font file has the bundled Libertinus face of its weight and slant stand in. Each counts as an error in Proof and leads to its place. Only an export marked final refuses to run while one stands, and says why.
- The first page is a recto and stands alone right of the gutter. After it, pages pair up as a printed book's do, with a 2 px gutter.
- The rail's tabs are book details, style, fonts, and images at the top, and agent, proof, and export at the foot. A tab shows once something is mounted on it.
- One zoom scale: contact sheets below fit, then fit, which shows a whole spread, then larger. Zooming out past fit becomes a contact sheet of every page in facing pairs, as a printed book has them: the first page alone on the right, a 2 px gutter within a pair and a wider gap between pairs, with a dot on a page that has a problem. A click zooms into its spread. Zooming back without a scroll or a click in between returns to the spread the zoom started from. Below fit there are four fixed sizes, so a sheet looks like one whatever the window. Fit itself follows the view below all of them, so a narrow window shows a whole spread. At Fit, Page Down and Page Up move one row of sheets, so a spread lands whole in the middle.
- A proof slip is a card under its paragraph on the panel color, as wide as the paragraph's column so no line end shows beside it and at least 380 px, or above the paragraph where the view's foot leaves too little room, so it never covers it. It has the paragraph's written text in the theme's font at 14 px, the two directive buttons, and at its foot the paragraph's style as a link. It follows the paragraph, and the view follows it to another page. Escape closes it, and so does a click on no text, or scrolling its paragraph out of view. It never stands above the title bar. The marked paragraph has a thin accent outline.
- The style link opens a short list in place of the directive buttons, with the text cut to a few lines so the card stays small: the theme's own style, then the custom styles for that kind of block, then alignment as four small buttons, then the way to the style's section. See [Selection](#selection). Escape closes the list first.
- A block in a list, a quotation, or a group kept together shows no directive buttons, since a directive line cannot stand there.
- Pages mark an error or an open placeholder with the same outline and a faint tint, and the one being looked at with a stronger tint. Typesetting warnings are marked only while the Proof tab is open or the Marks toggle is on, in a neutral grey outline with no tint, except the one being looked at. The toggle is remembered as a setting, so a proof pass need not hold the tab open. A warning about a whole page marks the page's text. On a page of the problem being looked at only its marks draw, so a page's tint and a paragraph's outline never nest. Only proof marks appear on pages, and only in this view. A dot on a contact sheet means an error or an open placeholder, never a warning.
- The accent stays on the action that moves on and on marks. A chosen segment and a chosen item are the raised grey with full ink, never the accent.

## Settings

A separate window after Zed's: categories on the left, Appearance, Writing, Publishing, Language and spelling, Agent, and Keys, then a search field, then rows. Search finds a row by its words or its category's name. A row is a label with one quiet line of description at the left and its control at the right, with thin dividers between rows. The left list ends in a link to `settings.json`. A choice applies at once. Toggles are small squared switches, not pills: on is a filled grey with a light knob at the right, darker in the light appearance so on and off read apart, off is an outline with a faint knob at the left. The problem line at the window's foot says when `settings.json` cannot be used or written, or its folder cannot be watched. The accent is six square swatches and a hex field.

## Hierarchy

Section labels, field labels, and rows keep the mockups' hierarchy: a small section label, then quiet field labels, then values. Inspector panels start with the small section label on a divider, then quiet field labels above their fields. A detail view starts with a quiet back link before its title.

## Binder

The Binder is the old sidebar, 216 px, on `Cmd+\`.

- From the top: the identity block on the raised color, then flat lists separated by thin dividers. Rows are 30 px, with no divider between them.
- A row is a lead column and a truncated title, and at the end a slot for its dot only when it has one, so the title takes the room otherwise. The lead is the chapter's number, or an icon where it has none: T for text with no heading, a list for contents, an open book for an unnumbered heading, a page for a plain file.
- Titles come from the text: the first heading, without its Markdown. A file with no heading shows its name without the ordering prefix. There is no section type behind any of this.
- The selected row uses the raised color and full text color. Other rows use the quiet text color, numbers and icons the faint one.
- With the keys in the Binder, the selected row has a 2 px accent bar at its left edge. Up and down move, Home and End jump, Enter or Escape return to the text.
- A row's edit button shows on hover, over the title's end. A file not in the book has one visible action, its plus. Notes and research fold at their labels and start open.
- A dragged row drops above the row under it, shown by a 2 px accent line at that row's top.
- The bar at the sidebar's foot holds the new section button at its right end.
- A dot means a broken reference or citation, or an open placeholder, nothing else. Page problems never show here. It is derived from the diagnostics list and never stored. The dots match the proof tab: filled accent for an error, hollow for an open placeholder.

## Dialogs and cards

- A dialog is a small card over the window, near the sidebar it came from, as in the mockups, with its heading and a close button. Templates list as a quiet icon, a name, and one quiet line of description, under quiet group labels, and the list fades at its foot while more is below.
- The question about a shared theme is a card near the sidebar, like the section dialog, above the proof slip. The recommended choice, a copy in the project, has the accent border and the keys as the card opens.
- A card that asks holds the keys: Tab and Shift+Tab stay inside it, Escape cancels it first, and when it goes the keys go back to what had them. The section dialog does the same.
- The search panel is a card at the text's left, after the Binder or at the window's gutter when the Binder is hidden. It takes its room as a panel does, so the text moves or narrows and no line is covered, and in a small window it narrows first. The find bar is a card over the text's corner. Neither is modal: no backdrop, so the text stays usable while one is open.
- Cards that centre over the text, the commit card and the palette, centre on the text column between the panels, not on the window. The search panel's three toggles have no legend: their names are their labels for assistive technology. Its list fades at its foot while more is below, as every list that scrolls on the panel colour does.
- When a card, float, or panel goes with the keys, they go back to the nearest place still on screen, or to what the mode is about, the text or the pages. Typed letters never go nowhere.

## The text

- The block gutter is empty at rest and while typing. Its type icons show when the pointer enters the left margin, the hovered block's in full ink, the others in the quiet color. Nothing else moves when it appears.
- A heading's icon is three short lines and its level. A dragged block drops above the block under the pointer, shown by a 2 px accent line, the same rule as a dragged sidebar row.
- A hidden comment takes no room, and a small speech mark stands right of the type icon on the row it was on. For a comment block it stands beside the next block that shows. Clicking it shows the comment in the flow, in the quiet color.
- A selection shows its text clean, with no markers revealed. Blank gaps between blocks show no selection.
- A label or style closing a heading or paragraph, such as `{#sec:intro}`, stays hidden until the caret is inside it.
- The scrollbar is a 6 px thumb in the pane's right edge, faint, and stronger when the pointer is over it.
- The source view uses one monospace on the same surface with no block backgrounds. Markers, comments, directives, and front matter are in the quiet color, headings bold, and code in the link color.
- Passages an agent changed show with the removed text struck through on a `removed` tint and the new text on an `added` tint, both in the book's type and at 22 percent, with small Accept, Reject, and Try again buttons below, at the text's left. The removed words come first and take a space of their own. A passage stays under review while the author types around its changed words, and leaves when the typing overlaps them. When there are any, the foot of the window shows how many in this chapter and elsewhere, with Next if any are elsewhere, then Accept all and Reject all, which act on all of them. Accept has accent text. The tints stay while typing, and nothing moves on a pause.
- A misspelled word has a 1 px line under it in the accent color at 60 percent, in the text and the source view. It takes no room and leaves as soon as its block is edited, until the next pause. Its menu uses the slash menu's rows: suggestions with a text icon, then a plus icon and the row that adds the word to the project's words. The word stays selected behind the menu.

## Floats

- The floating toolbar is one row on the panel color with a thin border and a shadow: marks, a thin separator, then quotation, lists, and link. A mark that is on inverts its button, text color behind panel color, as in the mockup. A mixed mark shows its icon in the quiet color. The mockup's `Tt` case command is not there. The small `Tt` icon that does stand there is the small capitals mark.
- It sits above the selection, or below it when there is no room above, and stays over the text up and down and inside the text column sideways. Only a selection taller than the window gets covered, at its top.
- A command that selects a placeholder opens no toolbar, and Escape hides it until the selection changes.
- Lists that float, the slash menu and a block's convert menu, start at the caret's or icon's left edge, not centered on it. Rows are 28 px on the panel color, with a faint icon that turns full ink on the chosen row. They show ten rows and scroll.

## Rich blocks

- A footnote's reference shows as a small mark: its number in the book, in superscript digits, in the link color, with no box. A citation or cross-reference is a chip: the engine's text in the link color on a 12 percent tint of it, with a 3 px radius. A key, label, or note the book cannot resolve is the same chip in the error color, showing its source. In Write the colors come from the appearance, so a chip stays legible in dark and light.
- A chip in brackets is one step for the caret and one key for Backspace. A short form, `@key` or `@sec:x`, shows its source while the caret is at its end, since it is being typed. A caret brought inside a chip shows its source.
- A note's text has no block of its own while a reference names it. It opens in a popover at its mark. A definition with no reference shows as a raw block, in the small size.
- Floats that have fields are cards on the panel color: a section label or a quiet title, then fields, then small 22 px buttons as in the agent panel. The choice that lets the work go on has accent text. Nothing is filled.
- A table at rest shows as a grid in the text: its caption above it, a rule above and below the header and below the last row, thin rules between rows, the header bold, and the columns' alignment. A click opens the editing grid over it at the same place, a card with its tools above, then its cells, then caption, label, and placement. As it opens and as its cell moves, the view scrolls so the card stands inside it. One field moves between the cells, and the others show their Markdown as written.
- A figure at rest shows centered, at most as wide as the column and three quarters of that tall, with its caption below. A click opens its card below the image and its caption, which the card edits, and the view scrolls to show it. A missing file is a tinted box with the error color's message.
- What is refused says why in the accent color, inside the float that asked, in the engine's words. A deleted reference offers to remove its note's text in a small card at the caret, which takes no focus and leaves with the next edit.

### Selection

The controls for a block's named style live at the proof slip's foot. The style's name opens a short list: the theme's own style first, then each custom style the block's kind may use as `.name`, then alignment as four buttons, left, center, right, and justify. Choosing a style writes `{.name}` as an ordinary edit of the text, one undo step, and choosing the theme's own takes it off. An alignment picks or makes a named style `base-align`, such as `body-center`, based on the style the block has now, and pressing it again takes it back off. So there are no per-block settings, and a style made here shows in the Style tab like any other. A style held back by the question about a shared theme follows once it is answered, and leaves with it when it is cancelled. Blocks that take no `{.name}`, such as a quotation, go straight to their style's section. The list's last row goes there too.

### Images

A row is a 44 px thumbnail on the raised color, the path as the document writes it, and one quiet line with the size and how often the book uses it. A file the text names that is not there shows the images icon and "File missing" in the accent color. A click goes to the first use. Insert image stands at the panel's foot.

## Keyboard

Only a keyboard's focus shows: a 2 px accent ring inside the control's edge, drawn as an inset shadow over any border, so nothing moves and nothing fills. A control the app gives the keys as a view opens, where Enter acts, shows its ring too: "Make this a project", the shared theme question's recommended choice, Export's first run button, and the result's Open. Tab and Shift+Tab walk the controls, and Enter or Space click. The window's regions keep one order, as a loop: the title bar, then the panel (the rail's in Publish, the Binder in Write), then the rail, then the pages and their status line or the text, then a right panel. A card open over the window comes first. In the text, Tab stays in the text, and Control+Tab leaves it. Rows of the contents list stay out of the tab order, since arrow keys walk them: the Binder holds one stop.

The keys: `Cmd+1` and `Cmd+2` for the modes, `Cmd+\` for the Binder, `Cmd+P` for chapters, `Cmd+Shift+P` for commands, `Cmd+J` for the agent, `Cmd+S` to write the pending save, `Cmd+Shift+S` for the commit card, `Cmd+Y` for the timeline, and `Ctrl+Shift+G` for the Git panel. `Cmd+K` stays on Link and `Cmd+Shift+G` on Find Previous.

## Saving state and conflicts

- The saving state shows only when something is wrong or a note is due: one faint line at the bottom left. It does not move the text or take focus.
- A note, such as a merge that happened, uses the quiet text color and leaves after 6 seconds, `tokens::NOTE_TIME`. Notes from Versions use the same line. A problem or a conflict uses the accent color and stays until it is over.
- A note about a change from outside names what made it: another program, the agent, a merge or pull of a branch, a switch to one, or a merge called off.
- A change from outside, from another program, a sync, or an agent, merges with the author's unsaved typing when they changed different sentences. When both changed the same sentence it is a conflict with both versions, never a blend of their words. A conflict Git reports stays one, even in different sentences.
- A conflict never opens a dialog. It is a line at the foot of the window with its choices as small raised buttons, and the text keeps the focus and keeps taking keys. A modal prompt would catch keys typed while it appeared, and agents can cause many of these. The keys are Ctrl+Cmd+M, T, and B.
- Above the line a small card shows what the other side wrote against the author's words in the book's type, with the timeline's tints: the author's words that would go are struck through on the `removed` tint, and theirs are on the `added` tint. The passage has a thin accent line under it in the text. During a Git merge the line names the branch and the text goes to the passage, since the author asked for this and is looking at it. A conflict from another program does not move the caret.
- See both is not settled while its marker comments stand. They show in the flow as quiet monospace lines of their words, never in the margin, until the caret comes to one. During a merge the foot says so in the accent color with Keep mine and Take theirs, which take the markers and one side out as one undo step, as Ctrl+Cmd+M and T do. Reopening the app in the middle of such a merge says so again. The marker lines name the two sides in the book's language: "yours" or "deins", and the branch for a Git merge, the agent, or another app. Markers in an older wording still count. The card names it too, and opens by itself only once they are gone.
- The conflict card keeps a space between struck words and the words that replace them, as the timeline does.
- A note about a kept copy names its folder, `kept`, with a quiet Show in Finder link to the copy.
- What fails beside saving shows on the same line until it is over: a folder that cannot be watched, a recovery copy that cannot be restored or written, a change that cannot be merged, a chapter list that cannot be written, local history that cannot be kept, a new project that could not become a repo, the app's own state that cannot be saved, theme fonts Write cannot use, and an agent conversation that cannot be kept for the next start or earlier ones that cannot be read.

## Rail panels

The panels of Publish's rail. See [Publish](#publish) for the tabs.

### Theme panels

- The Style tab is the theme picker, a search field, and a list of the theme's 30 sections under quiet group labels, with a section's main value at the end of its row when it has one. A section opens as its own view with a quiet back link. Tokens and the theme's source are entries in the same list. A section with no designed panel yet opens as JSON in the same place, with a labelled field that shows `{}` when the theme sets nothing there, and the default below it, wrapped. One with a panel has a quiet JSON link at the bar's end. The Fonts row goes to the Fonts tab.
- A field is a label row, then its control. The label row has the label at the start, and at the end what the value comes to when it is a token (quiet), a small `▾ $` link when the field takes tokens, and a quiet Reset link only when the theme itself sets the property. Values the theme inherits show in the faint text color, its own values in the full one.
- A token list opens inline below its field, never as a floating menu, so it cannot be clipped by the narrow panel. Colors have a swatch left of the field.
- The engine's refusal shows under the field in the accent color. A field the engine refused keeps what was typed.
- The picker's menu hangs from its bar across the panel's whole width, lists the project themes, then the shared ones, under quiet group labels in sentence case, and keeps a `⋯` on each shared row for rename, duplicate, and delete. While more rows are below, its foot fades. Deleting asks first, in the system's dialog.

### Proof

Above the list a quiet line counts errors, warnings, and open placeholders, leaving out any there are none of. The list groups by chapter in book order, with what belongs to no chapter, such as settings and themes, first under the project's name. A row is a dot, a quiet line with the kind and the line number, then the message in up to two lines, and as high as that needs. A message names a page as the status line does: by its place in the file, with the printed number in brackets when it differs, "page 14 (3)". An error, or the row being looked at, shows its whole message. A click goes to the problem on the pages and `Cmd+]` and `Cmd+[` walk the list. The row being looked at is raised, and Write opens at that place next.

### Export

The panel's section label is Export. It lists each named export as a collapsed card: name, then one quiet line of its output and switches, then the run button, Export, with accent text. `Cmd+E` gives the keys to the first run button. A project that names no export offers one built-in PDF in the same card with a quiet line saying so, and a quiet Save to `druck.yaml` link after Export. Nothing is written until the author saves it or changes one of its values, which makes it a named export called `pdf`. A card opens to its theme, output, and bleed fields and its three switches. Progress is three quiet segments, one per stage, with the stage's name above and Cancel beside it. When a run ends the keys go to the result's Open button, unless the author went on to the text.

### Agent

- It has no chrome until enabled: no rail tab, menu item, palette command, or key row. `Cmd+J` with a selection opens a one-line prompt card, 420 px, under the selection, which stays tinted, with the Enter sign at its right. Without one it opens a chat card, 300 px and at most 520 px high, at the bottom right, over the text. Closing it keeps the running turn, and the foot then says it is working, with Stop. In Publish, it is also a rail tab, and `Cmd+J` goes there.
- A waiting tool approval opens the chat card, and an accent dot on the Write switch marks it when the author is elsewhere.
- Margin notes have a thin accent line at their left, a title in full ink, one quiet line, a suggestion in the theme's font if there is one, then small buttons. Their passage has a quiet line under it, unlike a misspelled word's accent one. They stand beside their passage, in the margin's one flow. A note whose text is gone stands at the top of the margin, says so, and offers only Dismiss.
- A chapter in the chat card's turn summary goes by its title, and its rows say only where and how many lines, with Show, which goes to the words in the text. Other files go by their path and keep their diff rows.
- The chat card's count is labelled as this turn's. The review foot counts everything still under review. It stands at the foot's left, before any saving note, so a note coming and going moves nothing.
- Warnings stay out of what a prompt carries by default: only errors do.
- The setup lines fold into one quiet line after the first turn, unless the CLI needs the author. The chapter's chip shows its title, while the prompt names its file for the CLI. No costs show.
- Replies show as plain text. The author's prompts sit on the raised color. A tool call is one quiet line with its name and target, collapsed until clicked, with a word at its end only when it waits, was denied, or failed.
- Diffs in approvals use Menlo at the small size, one row per line with the line number, a sign, and the text. Added and removed rows are tinted at 14 percent in the `added` and `removed` colors.
- Buttons are 22 px high with a thin divider border and quiet text. The choice that lets the work go on has accent text. Nothing is filled and nothing is a pill. Chips are 22 px with a 4 px radius on the raised color, and end in a close icon.
- The setup text names an API key as the way to set Claude Code up and says what the login line reports. It never offers a login of its own.

## Versions

- The commit card is a card at the top of the text with no backdrop: a section label, a message field, the changed chapters with word counts, then the buttons. Only Markdown shows word counts: other text says "changed". A file that moved is one row where it went, "moved". A chapter of the book is named by its title, anything else by its path from the book's folder. A list longer than twelve rows fades at its foot until scrolled to the end. The card and the panel word a change the same way: `+3, −1 words`. Commit has accent text once there is a message and something to commit, and quiet text before, in the card and the panel. The same holds wherever an action is in the accent: Export while another run is going, and the agent's Send while its prompt is empty, are quiet. It names unreviewed agent changes in the accent color. Escape closes it and returns the keys, and `Cmd+Enter` commits from anywhere on it. A typed message stays until a commit succeeds, through Escape and the timeline, and opening the timeline closes the card. After a commit a quiet line at the window's foot names its short hash for 6 seconds.
- The timeline is a strip at the foot of the window, hidden until asked for, with the current branch at its left and at its right a faint hint of its keys and the sync state, which, like the panel's, shows nothing against a remote until the first fetch of this run returns. Comment markup never shows: a changed comment shows its words, and an unchanged one nothing. Commits are tall marks in full ink, local history small faint ticks, and the position an accent mark. Above it the chapter shows at that point, with word differences on `added` and `removed` tints.
- The Git panel has the rail panels' form: section label, quiet group labels, a branch picker, the changes, a message, then history as rows with a quiet date. A commit's leading symbol, such as a gitmoji, stands apart from its cut title, so the ellipsis stays whole, in a column every history row keeps, so titles with and without one align. It uses Git's words. It takes 296 px beside the text, which re-centres. The branch list opens inline below the picker, never as a floating menu, as token lists do. A row is the branch's name, then Switch, Merge, and Delete, and the row itself does nothing on a click. Merge asks once, as "Merge now?" in the accent color, and so does Delete for an unmerged branch. A new branch becomes the current one.
- The timeline covers the window below the title bar. The marks stand evenly along the strip, oldest at the left and now at the right, since a time scale would crush a day's ticks against years of commits. The line above the text names the chapter, the moment, and what the mark is, with Restore passage, in accent text since Enter does it, and Restore chapter at its right end. Its keys: left and right walk the marks, Option with them walks the commits, Home and End go to the ends, up and down choose a passage, Enter restores the passage, Command+Enter the chapter, and Escape closes.
- A word difference is against the text as it is now: what that version held and now is gone is struck through on the `removed` tint, and what has been added since is on the `added` tint. The chosen passage has a thin accent line under it.
- The first commit in a folder with no repo asks once, in the card. From then on the card says so in one quiet line, with the same button. A folder in iCloud or Dropbox has the warning in the accent color in the card and the panel, and a new project there says it once at the window's foot.
