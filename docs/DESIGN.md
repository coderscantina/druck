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

## Agent panel

- The panel is the Agent tab at the bottom of the right rail, above Diagnostics. It has the inspector's width. The conversation scrolls, and the approval, the chips, and the prompt stay at the foot, so a waiting approval is always on screen. While one waits and another tab is open, the rail button carries an accent dot.
- Replies show as plain text on the panel. The author's prompts sit on the raised color. A tool call is one quiet line with its name and target, collapsed until clicked, with a word at its end only when it waits, was denied, or failed.
- Diffs use Menlo at the small size, one row per line with the line number, a sign, and the text. Added and removed rows are tinted at 14 percent in the `added` and `removed` colors, and the same colors tint the passages an agent changed in the galley at 20 percent. A deleted passage has no mark in the galley, only its row in the panel.
- Buttons in the panel are 22 px high with a thin divider border and quiet text. The choice that lets the work go on has accent text. Nothing is filled and nothing is a pill. Chips are 22 px with a 4 px radius on the raised color, and end in a close icon.
- The setup text names an API key as the way to set Claude Code up and says what the login line reports. It never offers a login of its own.
