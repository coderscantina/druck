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

