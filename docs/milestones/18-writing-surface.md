# 18: Writing surface

## Goal and prerequisites

The app becomes a writing tool first, as in [the desktop briefing](../DESKTOP_BRIEFING.md#write). Builds on the app as milestone 16 left it. This comes before milestones 19 to 21, which build on its modes, tokens, and settings. See [the design document](../DESIGN.md) and [the first mockup board](../mockups/01-surfaces.html).

## Deliverables

1. **Modes.** Write and Publish on `Cmd+1` and `Cmd+2`, with the switch at the top centre. Split and the live preview pane go. Layout keeps running on a long pause with nothing on screen.
2. **Tokens.** Neutral greys in dark and light, softly filled fields, and a user-set accent read from one place.
3. **Theme type on the app surface.** No sheet. Theme fonts, sizes, weights, spacing, and hierarchy, with colours following the appearance. Lines wrap at the book's measure or the author's length. Print lines leave.
4. **Bare page and Binder.** Chapter ticks that open as the contents on hover, a Binder on `Cmd+\`, and the word count and saving state as the design document says. The inspector leaves Write, and chapter notes become margin notes.
5. **Commands.** `Cmd+P` finds a chapter and `Cmd+Shift+P` runs a command. Everything the menus and removed chrome offered is reachable here.
6. **Quieter tools.** The block gutter shows only on entering the left margin. A setting turns off the toolbar on selection. Write reports only spelling and broken references or citations.
7. **Settings window.** After Zed's, over one user-level `settings.json`. Categories, search, rows, and a link to the file. Only changed values are written, and an invalid file is reported, never overwritten.
8. **Zoom and focus.** Zoom per mode, kept between runs. Typewriter scrolling and dimming the other paragraphs.

## Completion criteria

On a copy of the owner's real book: both appearances and two accents inspected on screen, in English and German. Typing stays under 16 ms median with focus features on. Entering Publish after a pause shows pages at once. A chapter can be reached and every former toolbar action run with the keyboard alone. Editing `settings.json` by hand takes effect while running, and the window shows it. The input method test from milestone 14 runs on the new surface.
