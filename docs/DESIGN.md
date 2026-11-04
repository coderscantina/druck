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

## Theme panels

- The Styles tab holds four panels behind a strip of four segments at its top: Styles, Tokens, Page, and Source. The Fonts tab has its own. Each panel starts with the small section label, and a detail view starts with a quiet back link before its title.
- A field is a label row, then its control. The label row has the label at the start, and at the end what the value comes to when it is a token (quiet), a small `▾ $` link when the field takes tokens, and an accent Reset link only when the theme itself sets the property. Values the theme inherits show in the faint text color, its own values in the full one.
- A token list opens inline below its field, never as a floating menu, so it cannot be clipped by the narrow panel. Colors have a swatch left of the field.
- The engine's refusal shows under the field in the accent color. A field the engine refused keeps what was typed.
- The theme picker and the specimen switch sit in the title bar before the canvas switch. The picker's menu hangs from its button, lists the project themes, then the common ones, and keeps a `⋯` on each common row for rename, duplicate, and delete. Deleting asks first, in the system's dialog.
- The question about a common theme is a card near the sidebar, like the section dialog. The recommended choice, a copy in the project, has the accent border.
