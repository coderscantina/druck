# 15: Engine amendment

## Goal and prerequisites

Give the CLI what the [desktop app](../DESKTOP_BRIEFING.md) and real book projects need, so `build.sh` in `~/Development/books/ttp404-gott-nicht-gefunden` is no longer necessary. Requires milestone 14. Delivers the [desktop amendment](../BRIEFING.md#desktop-amendment).

## Deliverables

- **Project folders.** `druck render ./book` and `druck check ./book` read `druck.yaml`: the metadata and settings front matter already takes, an ordered `chapters` list, and named exports. Chapter files have no front matter. Diagnostics name the real chapter file and line. An unlisted Markdown file in `manuscript/` warns and is excluded. A listed file that is missing is an error. Labels are book-wide. Project themes in `themes/<name>/` win over named user themes.
- **Comments.** `<!-- -->` comments are skipped, at block level and inline. All other raw HTML stays an error.
- **Inline marks.** `~~strike~~`, `^sup^`, `~sub~`, `[x]{.underline}`, `[x]{.smallcaps}`, in PDF and EPUB. Theme settings for raise and size follow the footnote marker's pattern. Small caps reuse the existing synthetic path. Update the "not supported" line in authoring.
- **Placeholders.** `{todo: text}` shows in drafts in a visible style. An export marked `final` fails at its location while any remain.
- **Named exports.** `exports` in `druck.yaml` with `theme`, output path, format from the extension, `bleed`, `crop-marks`, `draft`, and `final`. `druck render ./book --export print`. A fixed set of fields, not a scripting system. A single-file render ignores this.

## Completion criteria

Existing samples render byte-identical unless they use new syntax. Tests cover each diagnostic at its real file and line, comment placement, every mark in both outputs, and export selection. A copy of the ttp404 book as a project with `druck.yaml`, with its comments intact, renders with the same page count as its current build; inspect pages. Update [authoring](../AUTHORING.md), [themes](../THEMES.md), the schema, and [decisions](../DECISIONS.md).
