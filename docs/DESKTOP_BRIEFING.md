# Briefing: Druck desktop

Settled with the owner on 2026-11-03 and redirected on 2026-11-05. The dated decisions are in [the first decision](DECISIONS.md#2026-11-03-desktop-app) and [the redirection](DECISIONS.md#2026-11-05-writing-first-direction). The mockups the owner chose from are in [mockups](mockups/).

## Goal

The best Markdown writing experience for self-publishing authors of books, papers, and articles, and a calm way to turn the manuscript into a finished book. It is native: Rust only, no Electron, no webview, no JavaScript. It uses the same engine as the CLI. There is never a second layout implementation.

Writing comes first. Publishing is a separate step the author moves to, not a preview that runs beside the text.

The toolkit is `gpui`, Apache-2.0. Zed's `editor` crate is GPL-3.0, so no code may be copied from it. Druck stays MIT.

Long-form authors come first: a decision favours someone writing 80,000 words. The starters are book, paper, and article. There is no document kind in the engine, and the engine and CLI keep every capability they have, including reports and offers.

## Sequence

The app as it stood on 2026-11-04 is the base. Nothing has been published, so the first public release is this direction.

1. [Writing surface](milestones/18-writing-surface.md): the two modes, the bare page, theme type on the app surface, the design tokens, settings, zoom, and focus. Comes first, since the others build on it.
2. These may run in parallel: [Publish view](milestones/19-publish-view.md), [Writing partner](milestones/20-writing-partner.md), and [Versions](milestones/21-versions.md).
3. [Writer beta](milestones/22-writer-beta.md): the gate for the first public release.
4. [Desktop 1.0](milestones/23-desktop-1-0.md): every theme section hand-designed, and the other platforms.

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
.druck/           the agent's margin notes
```

Unknown files, `.claude/`, and `CLAUDE.md` are left alone.

- Chapter order is the list in `druck.yaml`. A Markdown file in `manuscript/` the list does not name is excluded and flagged: a "Not in book" group in the contents, a warning in `druck check`. A listed file that is missing is an error.
- `druck render ./book` finds the manifest. Diagnostics report the real chapter file and line.
- Footnote and cross-reference labels are book-wide.
- The example of a real project is `~/Development/books/ttp404-gott-nicht-gefunden`: 27 chapter files, 45,404 words, 203 pages, and 138 commits. Its `scripts/build.sh` shows what this model replaces. Read it, do not modify it.

## Two modes

The window is in Write or Publish, on `Cmd+1` and `Cmd+2`, with a switch at the top centre. There is no split view and no live preview. Both modes edit the same text, so a fix in one is there in the other.

Layout keeps running on a long pause in Write, with nothing on screen, so Publish opens at once and chips show the engine's numbers. While text is invalid mid-edit, pages keep the last good result.

## Write

Markdown text is the truth. Every edit is a change to the text, and syntax markers are decoration. Blocks that were not touched stay byte-identical. Constructs without a rich editor stay raw text.

- **Bare page:** by default only the text and the mode switch are on screen. Chapters are ticks at the left edge that open as the contents on hover. `Cmd+P` finds a chapter and `Cmd+Shift+P` runs any command.
- **Binder:** `Cmd+\` shows or hides a slim contents list on the left. The app remembers it, and Settings shows the same value.
- **Fidelity:** the text uses the selected theme's fonts, sizes, weights, spacing, and heading hierarchy. These limits are deliberate, for the sake of writing: colours follow dark or light, and there is no sheet, justification, hyphenation, or pagination. Lines wrap at the book's measure by default, or at the author's own length.
- **Behaviours:** live formatting as you type with markers hidden when the cursor leaves them, a slash menu, a toolbar on selection that a setting can turn off, a block gutter that shows only when the pointer enters the left margin, and a per-chapter source view. Selection crosses blocks; text stays continuous.
- **Rich blocks:** footnotes (popover at the reference, generated labels), citations and references (picker, chips), tables and images (grid editing; assets copy into the project). Directives and index marks use source view.
- **Comments:** a leading `<!-- -->` shows as one line above the chapter's title that unfolds in place, and a later one in the margin beside its passage, cut to six lines.
- **Focus:** typewriter scrolling and dimming the other paragraphs, both settings. Typing stays under 16 ms median.
- **Saving:** autosave after typing stops, watch the folder, reload external changes, merge when they do not overlap unsaved edits, ask when they do. `Cmd+S` only writes the pending save at once.
- **What it reports:** spelling and broken references or citations, in place. Page problems wait for Publish.
- **Also:** project-wide search, a word count from the existing statistics, the system spell checker behind a small seam.

## Publish

The manuscript's pages, painted natively from the engine's page output, never through PDF. Pages always show the whole document.

- **Spread and rail:** spreads in the centre, and a right rail with book details, style, fonts, images, and at its foot proof and export.
- **Whole book:** zooming out past fit becomes a contact sheet of every page. Clicking a page zooms back into its spread. It is one zoom scale, with no extra mode.
- **Proof slip:** clicking a line opens a small card at its paragraph with the text in the theme's font. Pages reflow on a pause. The slip also offers the directives that exist: a page break before, and keep together. Its foot names the paragraph's style, which opens a short list of the theme's own style, the custom styles, and alignment as named styles, and a way to that style's section.
- **Proof:** the rail's proof tab lists the book's diagnostics, the typesetting warnings among them, with a key to walk them. Pages mark the affected lines.
- **Export:** named exports, from the rail and a button in the title bar.

## Theme editor

Themes remain strict versioned JSON without inheritance. Common themes in the user config folder get full create, read, update, delete. Customizing one inside a project copies the JSON and its fonts and images to `themes/<name>/`; a project theme wins over a common one of the same name. Only changed fields are written, and partial themes stay partial.

The Style tab is a searchable index of sections, then one section at a time with a back link. All 30 top-level sections of the schema get a hand-designed panel before 1.0. The beta ships the sections authors reach for most, and the rest open as JSON in the same panel. The bundled specimen covers every element for a theme with no document open.

## Versions

Two tiers, so the small changes stay out of Git.

- **Local history:** automatic, per file, outside the project in the app data folder, as in JetBrains IDEs. A snapshot on a pause and a labelled one before each agent turn. It keeps 30 days, and it works for every book, single files included.
- **Git:** deliberate versions, in Git's own words: commit, branch, push, pull. The repo is a normal one, and the app commits only when the author asks.
  - `Cmd+Shift+S` opens the commit card: changed chapters with word counts, a message, commit, and commit and push.
  - `Cmd+Y` opens the timeline, hidden until then: commits as tall marks, local history as small ticks. Differences show word by word in the book's type. A chapter or a passage can be restored.
  - `Ctrl+Shift+G` opens the Git panel: changes, message, branches, and history. It is a right panel in Write and a rail tab in Publish.
  - Branches can be created, switched, deleted, and merged. An overlapping merge uses the existing conflict line.
- **When a book becomes a repo:** new projects start as one, with a warning if the folder is in iCloud or Dropbox. An existing folder without one is asked once, on its first commit. A single Markdown file has local history only.
- **Tools:** gitoxide for local work, so authors do not need Git installed. Push and pull call the installed `git`, using the author's own keys and credential helpers. The app stores no credentials. Cloning and creating remotes stay with the author's own tools.

## Agent

Spawn the user's installed `claude` CLI as a child process over `stream-json`, behind one module with a version check. Project `CLAUDE.md`, skills, and MCP then work as in a terminal. It stays off until enabled in Settings, and leaves no trace on screen until then.

- **Way of working:** like a modern IDE. It can write a whole chapter, refine chapters or passages, and change a selection.
- **`Cmd+J`:** with a selection, a prompt opens at the selection and works on it. Without one, a small chat card opens for book-wide work.
- **Edits land in the files**, written by the CLI's normal tools, and show in the text as tinted changes with Accept and Reject per passage, and Accept all. A local history label before each turn undoes the turn. Export and commit warn about unreviewed changes but do not block.
- **Advice changes nothing:** margin notes beside a passage, kept in `.druck/` and anchored by quoted text, with Apply, Reply, and Dismiss.
- **Credentials:** the app holds none and never handles tokens. Anthropic's docs bar third parties from offering claude.ai login or routing requests through Free, Pro, or Max credentials. Before a public release documents a subscription login, confirm in writing with Anthropic that spawning the user's own CLI with one is allowed. Until then API key setup is the documented path, and this does not block the release, since the agent is off by default.
- **Sessions:** one resumable session per project. The chapter, selection, and diagnostics pass as context.
- Rendering stays fully offline. Only this optional agent and `git push` need a network.

## App shell

- **Look:** neutral greys, thin dividers, compact rows, and forms with a small section label, a quiet group label, and the field label above a softly filled field. Dark and light both ship.
- **Accent:** the author picks it, from presets or a hex value. Oxide red is the default. It colours only the caret, selection, marks, and the action that moves on.
- **Zoom:** `Cmd++`, `Cmd+-`, and `Cmd+0` act on the current mode, and each mode remembers its level. Write zoom scales the text and keeps the characters per line.
- **Settings:** a separate window after Zed's: categories on the left, a search field, rows with a label, a description, and a control, and a link to the file. One user-level `settings.json` is the source, and only changed values are written. What belongs to a book stays in `druck.yaml`, so there is no per-book scope.
- **Details:** title, subtitle, authors as text, date, language, cover, bibliography, and free `meta` rows with suggestions such as ISBN and publisher.
- **New section:** inserts plain Markdown from a template list. No section type exists.
- The UI ships in English and German from the first public release, with all strings in one table.
- Full keyboard use, and AccessKit through gpui where it works. Record the gaps.
- macOS first, with platform code behind small seams; Linux and Windows follow. MIT, one Cargo workspace, signed GitHub downloads.

## Boundaries

No section types, no regex or script checks in the manifest, no per-block overrides, no structured person names. Style checks stay the author's own scripts. No commit happens without the author asking, and no Git hosting accounts.
