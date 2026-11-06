# Writing UX boards, 2026-11-06

The HTML boards the owner rated. [The design document](../../DESIGN.md) holds the settled rules, and its ratings section says what lost. Open a file in a browser; each board is a `<section class="board" id="...">`. These are the winners to build from:

| Surface | File | Board |
| --- | --- | --- |
| Directives | `text2.html` | `dir-mix` |
| Placeholders, chip display | `text2.html` | `ph-chips2` |
| Placeholders, ghost display | `text2.html` | `ph-ghost2` |
| Inline style | `text.html` | `st-a` |
| Heading ticks | `nav2.html` | `tick-mix` |
| Path bar and overview | `nav2.html` | `files-system` |
| Bookshelf | `nav3.html` | `spine-abstract` |
| Supporting material, content and pins | `support2.html` | `sup-mix` |
| Supporting material, navigation | `support3.html` | `nav-trail` |

`sup-mix` shows the rail, panel content, pins, and library, but its open-item tabs lost to `nav-trail`'s trail and jump field. The boards use the app's tokens from `desktop/src/tokens.rs` through `druck.css`, and their sample book is invented.
