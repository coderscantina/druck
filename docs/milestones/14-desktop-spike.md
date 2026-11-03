# 14: Desktop spike

## Goal and prerequisites

Prove the risky parts of the [desktop app](../DESKTOP_BRIEFING.md) before shaping the phase 2 crate. Some of this code may be thrown away. It may use private engine internals through a temporary library target.

The target corpus is `~/Development/books/ttp404-gott-nicht-gefunden/buch-1-ttp404/manuscript` (27 files, 203 pages). Read it, never write to it. Copy it to a scratch folder for edit tests.

## Layout

The root package stays `druck` with its binary, and gains a library target exposing what the spike needs. A `desktop/` crate named `druck-desktop` joins a Cargo workspace. Engine-free logic lives in plain modules with unit tests, and gpui stays in the view modules.

## What it must prove

1. **Editor in gpui:** a custom text element with cursor, selection across blocks, undo, and IME input (German dead keys and composition), on a chapter-length document.
2. **Lossless Markdown editing:** hidden syntax while untouched blocks stay byte-identical across edit, save, and reload, including directives, tables, citations, and comments.
3. **Live page preview:** the engine's page output painted natively in a pane, refreshed after edits on a background thread.
4. **Theme round trip:** a style property changed in the inspector, written to the theme JSON as a partial change, with galley and preview updating.
5. **Agent protocol probe:** one streamed turn and one approved edit through the `claude` CLI over `stream-json`. No panel design.

## Completion criteria

- On the 203-page corpus, typing never waits on layout, and the preview refreshes within about 1 second of a pause. Measure and record the numbers.
- Unit tests for buffer edits, block segmentation, and byte-identical round trips across every `samples/*.md` and every corpus file.
- A running window, inspected on screen. IME composition with a German layout is exercised by a person if automation cannot do it; say so if it was not.

If gpui fails the editor test, stop and report rather than switching toolkit. Record what the spike teaches for the phase 2 crate in [decisions](../DECISIONS.md).
