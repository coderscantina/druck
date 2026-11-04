# Desktop design

Read this before UI work. When the owner settles a visual rule, write it here.

## Direction

Follow the structure and density of the owner's seven mockups, which are not committed: three panes, a right tab rail, thin dividers, compact rows. Do not copy them: Druck sets its own tokens, icon set, and details. Dark and light both ship.

The page sheet shows the theme's own fonts and colors. App chrome never styles document content.

## Tokens

Colors, spacing, and type live in one table in the desktop crate. Views never hardcode a value. Exact values are open until the first visual pass.

## Rules so far

- Section labels, field labels, and rows keep the mockups' hierarchy: a small section label, then quiet field labels, then values.
- The block gutter is quiet until a block is active or hovered.
- A sidebar dot means problems or open placeholders, nothing else.

## Sidebar

- From the top: the identity block on the raised color, then flat lists separated by thin dividers. Rows are 30 px, the sidebar 256 px.
- A row is a lead column, a truncated title, and a slot for its dot at the end. The lead is the chapter's number, or an icon where it has none: T for text with no heading, a list for contents, an open book for an unnumbered heading, a page for a plain file.
- Titles come from the text: the first heading, without its Markdown. A file with no heading shows its name without the ordering prefix. There is no section type behind any of this.
- The selected row uses the raised color and full text color. Other rows use the quiet text color, numbers and icons the faint one.
- A row's edit button shows on hover. A file not in the book has one visible action, its plus. Notes and research fold at their labels and start open.
- A dragged row drops above the row under it, shown by a 2 px accent line at that row's top.
- The bar at the sidebar's foot holds the new section button at its right end.

## Dialogs and panels

- A dialog is a small card over the window, near the sidebar it came from, as in the mockups, with its heading and a close button. Templates list as an accent icon, a name, and one quiet line of description, under quiet group labels.
- Inspector panels start with the small section label on a divider, then quiet field labels above their fields.
- The launch screen is one small card of rows on the canvas color: open, then the starters.

## Slots

The window has three places to mount a view without touching the shell, listed in `desktop/src/app.rs`: `toolbar` in the title bar before the canvas switch, `footer` in the strip under the canvas, and `overlays` over the window. Inspector tabs take a panel with `InspectorView::mount`.

## Shell panels

- The diagnostics list groups by chapter in book order, with what belongs to no chapter, such as settings and themes, first under the project's name. A row is a dot, a quiet line with the kind and the line number, then the message in up to two lines. The dots match the sidebar's: filled accent for an error, filled quiet for a warning, hollow for an open placeholder. A click opens the chapter at the line.
- A sidebar dot is derived from that list and never stored: a problem dot if the chapter has an error or a warning, else a placeholder dot if one is open.
- The export panel lists each named export as a collapsed card: name, then one quiet line of its output and switches, then the run button. A card opens to its theme, output, and bleed fields and its three switches. Progress is three segments, one per stage, with the stage's name above and Cancel beside it.
- The search panel and the find bar are cards over the galley's corner, not modal: the card draws no backdrop, so the pages stay usable while it is open. Settings is a modal card with a backdrop, like the section dialog.
- The footer's word count is right-aligned and faint, with a thin space between thousands.
- Every string is in English and German. A German label must fit the same width as its English one: the sidebar is 256 px and the inspector 296 px, so prefer the short word.
- With the keys in the sidebar, the selected row has a 2 px accent bar at its left edge. Up and down move, Home and End jump, Enter or Escape return to the galley.


## Editor commands

- The floating toolbar is one row on the panel color with a thin border and a shadow: marks, a thin separator, then quotation, lists, and link. A mark that is on inverts its button, text color behind panel color, as in the mockup. A mixed mark shows its icon in the accent color. There is no `Tt` button.
- It sits below the selection, or above it when there is no room below, and stays over the galley up and down and inside the window sideways. Only a selection taller than the galley gets covered, at its top.
- A command that selects a placeholder opens no toolbar, and Escape hides it until the selection changes.
- Lists that float, the slash menu and a block's convert menu, start at the caret's or icon's left edge, not centered on it. Rows are 28 px on the panel color, with a faint icon that turns accent on the chosen row. They show ten rows and scroll.
- The block gutter sits inside the sheet, left of the text, as in the mockups, so the sheet is wider than the text by the gutter and a right margin. Every block's type icon shows in the quiet color at low opacity, and the active or hovered block's in full ink.
- A heading's icon is three short lines and its level. A dragged block drops above the block under the pointer, shown by a 2 px accent line, the same rule as a dragged sidebar row.
- A hidden comment takes no room, and a small speech mark stands right of the type icon on the row it was on. For a comment block it stands beside the next block that shows. Clicking it shows the comment in the flow, in the quiet color.
- A selection shows its text clean, with no markers revealed. Blank gaps between blocks show no selection.
- The scrollbar is a 6 px thumb in the pane's right edge, faint, and stronger when the pointer is over it.
- The source view uses one monospace on the same sheet with no block backgrounds. Markers, comments, directives, and front matter are in the quiet color, headings bold, and code in the link color.
- The notes panel is the inspector's Notes tab: a section label, then a field for each note holding its text with plain line breaks.

## Saving state

- The saving state is one line at the left of the footer: faint "Saved", "Edited", or "Saving…". It does not move the text or take focus.
- A note, such as a merge that happened, uses the quiet text color and leaves after 6 seconds. A problem or a conflict uses the accent color and stays until it is over.
- A conflict never opens a dialog. It is a line in the footer with its choices as small raised buttons, and the galley keeps the focus and keeps taking keys. A modal prompt would catch keys typed while it appeared, and agents can cause many of these. The keys are Ctrl+Cmd+M, T, and B.

## Theme panels

- The Styles tab holds four panels behind a strip of four segments at its top: Styles, Tokens, Page, and Source. The Fonts tab has its own. Each panel starts with the small section label, and a detail view starts with a quiet back link before its title.
- A field is a label row, then its control. The label row has the label at the start, and at the end what the value comes to when it is a token (quiet), a small `▾ $` link when the field takes tokens, and an accent Reset link only when the theme itself sets the property. Values the theme inherits show in the faint text color, its own values in the full one.
- A token list opens inline below its field, never as a floating menu, so it cannot be clipped by the narrow panel. Colors have a swatch left of the field.
- The engine's refusal shows under the field in the accent color. A field the engine refused keeps what was typed.
- The theme picker and the specimen switch sit in the title bar before the canvas switch. The picker's menu hangs from its button, lists the project themes, then the common ones, and keeps a `⋯` on each common row for rename, duplicate, and delete. Deleting asks first, in the system's dialog.
- The question about a common theme is a card near the sidebar, like the section dialog. The recommended choice, a copy in the project, has the accent border.
