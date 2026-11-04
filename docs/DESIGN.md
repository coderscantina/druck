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

## Saving state

- The saving state is one line at the left of the footer: faint "Saved", "Edited", or "Saving…". It does not move the text or take focus.
- A note, such as a merge that happened, uses the quiet text color and leaves after 6 seconds. A problem or a conflict uses the accent color and stays until it is over.
- A conflict never opens a dialog. It is a line in the footer with its choices as small raised buttons, and the galley keeps the focus and keeps taking keys. A modal prompt would catch keys typed while it appeared, and agents can cause many of these. The keys are Ctrl+Cmd+M, T, and B.
