# Briefing: Druck desktop

Settled with the owner on 2026-11-03. The dated decisions are in [decisions](DECISIONS.md#2026-11-03-desktop-app).

## Goal

A native desktop composer and a complete theme editor for Druck, in a Rust-only codebase: no Electron, no webview, no JavaScript. It uses the same engine as the CLI. There is never a second layout implementation.

The toolkit is `gpui`, Apache-2.0. Zed's `editor` crate is GPL-3.0, so no code may be copied from it. Druck stays MIT.

It serves books, reports, articles, and offers equally. There is no document kind in the engine; starter templates differ.

## Sequence

1. [Spike](milestones/14-desktop-spike.md): prove the risky parts. If gpui fails the editor test, stop and report; do not switch toolkit.
2. [Engine amendment](milestones/15-engine-amendment.md): project folders, comments, inline marks, placeholders, named exports.
3. [Milestone 10](milestones/10-release-acceptance.md): accept and release the CLI with them.
4. [Phase 2](RUST_CRATE_API_PHASE_2.md): the public crate, with the app as its first client.
5. The app, on the crate. Public betas ship once the composer, projects, exports, and core theme sections work. 1.0 requires every theme section hand-designed.

## Project model

A single Markdown file keeps working. A project folder holds:

```
druck.yaml        metadata, theme, ordered chapter list, named exports
manuscript/       chapter files, without front matter
notes/  research/ open in the same editor, never built
themes/<name>/    customized themes with their assets
assets/           images
references.bib
build/            output
```

Unknown files, `.claude/`, and `CLAUDE.md` are left alone.

- Chapter order is the list in `druck.yaml`. A Markdown file in `manuscript/` the list does not name is excluded and flagged: an "Not in book" group in the sidebar, a warning in `druck check`. A listed file that is missing is an error.
- `druck render ./book` finds the manifest. Diagnostics report the real chapter file and line.
- Footnote and cross-reference labels are book-wide.
- The example of a real project is `~/Development/books/ttp404-gott-nicht-gefunden`: 27 chapter files, 45,404 words, 203 pages. Its `scripts/build.sh` shows what this model replaces. Read it, do not modify it.

## Editor

Markdown text is the truth. Every edit is a change to the text, and syntax markers are decoration. Blocks that were not touched stay byte-identical. Constructs without a rich editor stay raw text.

- **Galley:** one chapter at a time on a sheet in the theme's fonts, sizes, and alignment. A single-file document is split in view at level 1 headings. Lines use toolkit layout while typing, with print-composer lines on a toggle.
- **Canvas:** Write, Pages, or split. Clicking in one locates the same place in the other. Pages always show the whole document, painted natively from the engine's page output, never through PDF. While text is invalid mid-edit, pages keep the last good result and diagnostics show.
- **Behaviours:** live formatting as you type with markers hidden when the cursor leaves them, a floating toolbar, a slash menu, a block gutter with type icons, convert, and drag, and a per-chapter source view. Selection crosses blocks; text stays continuous.
- **Rich blocks:** footnotes (popover at the reference, generated labels), citations and references (picker, chips), tables and images (grid editing; assets copy into the project). Directives and index marks use source view.
- **Comments:** a leading `<!-- -->` shows in a chapter notes panel, inline ones as a gutter marker.
- **Saving:** autosave after typing stops, watch the folder, reload external changes, merge when they do not overlap unsaved edits, ask when they do. The app makes no commits.
- **Also:** project-wide search, a word count from the existing statistics, a diagnostics list that jumps to the source, the system spell checker behind a small seam.

## Language additions

See [the amendment](milestones/15-engine-amendment.md). No per-block style attributes: a one-off change creates or picks a named custom style, attached with the existing `{.name}`.

## Theme editor

Themes remain strict versioned JSON without inheritance. Common themes in the user config folder get full create, read, update, delete. Customizing one inside a project copies the JSON and its fonts and images to `themes/<name>/`; a project theme wins over a common one of the same name. Only changed fields are written, and partial themes stay partial.

All 30 top-level sections of the schema get a hand-designed panel before 1.0. Betas ship the core (styles, fonts, tokens, page geometry) plus a JSON view for the rest. The preview shows the open document, jumping to a block using the edited style, or a bundled specimen covering every element.

## App shell

Follow the layout of the seven mockups, with Druck's own identity, in dark and light.

- Left: document identity, then the contents list. A dot on a row means problems or open placeholders there. It is derived, never stored.
- Centre: the canvas.
- Right: an inspector with a tab rail: selection, styles, fonts, images, details, and at the bottom diagnostics and export.
- Details: title, subtitle, authors as text, date, language, cover, and free `meta` rows with suggestions such as ISBN and publisher.
- New section: inserts plain Markdown from a template list. No section type exists.
- The UI ships in English and German from the first public release, with all strings in one table.
- Full keyboard use, and AccessKit through gpui where it works. Record the gaps.
- macOS first, with platform code behind small seams; Linux and Windows follow. MIT, one Cargo workspace, signed GitHub downloads.

## Agent panel

Spawn the user's installed `claude` CLI as a child process over `stream-json`, behind one module with a version check. Project `CLAUDE.md`, skills, and MCP then work as in a terminal.

- The app holds no credentials and never handles tokens. Anthropic's docs bar third parties from offering claude.ai login or routing requests through Free, Pro, or Max credentials. Before any public release, confirm in writing with Anthropic that spawning the user's own CLI with a subscription login is allowed. Until then API key setup is the documented path.
- Tool approvals appear in the panel. The agent writes files like any external tool, and each turn's changes appear as a diff in the galley, revertible per hunk.
- One resumable session per project. The chapter, selection, and diagnostics pass as context.
- Rendering stays fully offline. Only this optional panel needs a network.

## Boundaries

No built-in Git, no section types, no regex or script checks in the manifest, no per-block overrides, no structured person names. Style checks stay the author's own scripts.
