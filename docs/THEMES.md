# Writing themes

A theme is one JSON file plus any local assets (fonts, images) it references. It controls the design of a document: element styles, page geometry, title blocks, headers, footers, and generated labels. Document authors change a small subset of it from front matter, see [authoring](AUTHORING.md).

## Named themes

Use `druck render article.md --theme latex-1`, or `theme: latex-1` in front matter, to select `latex-1.json` from your theme directory:

- macOS and other Unix systems: `$XDG_CONFIG_HOME/druck/themes` when `XDG_CONFIG_HOME` is an absolute path, otherwise `~/.config/druck/themes`.
- Windows: `%APPDATA%\druck\themes`.

Names contain ASCII letters, digits, hyphens, or underscores. An existing file with that exact name takes precedence: relative to the working directory for `--theme`, or the document directory for front matter. A value with an extension or path separator is always a file path, so `--theme ./latex-1` explicitly selects a local file. A missing named theme reports the searched JSON path. Assets resolve relative to the selected JSON file, including when it came from the shared directory. No network access is involved.

In a [project folder](AUTHORING.md#projects), a bare name looks in the project first:

1. `themes/<name>/theme.json` in the project folder.
2. An existing file with that exact name, as above.
3. `<name>.json` in the shared directory.

This holds for `theme` in `druck.yaml`, `theme` in an export, and `--theme`. A single-file document does not look in any `themes/` folder. Assets resolve relative to the selected JSON file here too, so a project theme keeps its fonts and images beside it. The desktop app uses the same lookup.

The reference themes prepared on 2026-10-07 are installed in `~/.config/druck/themes`: `coderscantina`, `latex-1`, `latex-2`, and `latex-3`. Their metadata and column instructions are in that directory's `README.md`. The three LaTeX references differ in body size, margins, title treatment, and columns, so they remain separate. They use bundled Libertinus. `coderscantina` uses static faces generated from the owner's installed Bad variable fonts; the JSON and its `fonts/bad` directory travel together. These personal themes are not bundled into the binary.

`druck check` loads and validates a theme, and `druck check --print-config` shows the resolved result. `druck render` uses it to typeset a document.

References: the [JSON Schema](../schema/theme.v1.schema.json) and the [bundled default theme](../themes/default.json).

## File basics

- `"version": 1` is required in every theme file. Other versions are rejected.
- `"$schema"` is optional and only a hint for editors. Point it at the schema file, for example `"../schema/theme.v1.schema.json"`.
- Unknown fields are errors. Field names are kebab-case.
- The bundled default theme is complete and is always the base. A theme may be partial and list only what it changes.
- There are no parent themes and no overlays. A document selects exactly one theme file.

## Resolution

Configuration resolves in this order, later layers winning:

1. Bundled default theme.
2. The selected theme file.
3. Front matter settings.
4. `--set` command-line overrides.

Merge rules:

- Objects merge by field. Arrays (`lists.bullets`, `page.wide`, title `slots`, `title-page.groups`, and each header and footer) replace the earlier array whole.
- An omitted field inherits the earlier value.
- `null` is accepted only where listed below. Anywhere else it is an error.

Every layer is validated on its own, so a later override never excuses an invalid earlier value. The merged result is validated again for things no single layer can show: undefined tokens, token cycles, missing font faces, and page geometry.

### Where null is allowed

| Field | Meaning of null |
| --- | --- |
| `pages.title`, `pages.first`, `pages.odd`, `pages.even` | Remove the variant, so the fallback order applies. |
| `pages.<variant>.header`, `pages.<variant>.footer` | No header or footer on that variant. |
| `page.text-width` | Prose uses the whole margin frame. |
| `page.column-change-spacing` | The column gap. |
| `tables.top-rule`, `tables.header-rule`, `tables.row-rule` | No such rule. |
| `inline.code.background` | No box behind inline code. |
| `highlight` | No highlighting: code blocks are set in the `code-block` color. |
| `fonts.<family>.<face>` other than `regular` | The family has no such face. |

`pages.body` itself cannot be null, and `fonts.<family>.regular` is required.

## Sections

| Key | Purpose |
| --- | --- |
| `version`, `$schema` | Format version and editor hint. |
| `document` | Defaults for document settings: `lang`, `title-page`, `toc`, `duplex`, `numbered-headings`, `numbering-depth`, `toc-depth`, `citation-style`, `draft`, `conformance` (see [accessibility and archiving](AUTHORING.md#accessibility-and-archiving)). |
| `fonts` | Font families and their files. |
| `images` | Named images used by title slots, the chapter ornament, and scene breaks. |
| `tokens` | Named fonts, sizes, spacing, and colors. |
| `page` | Page size, margins and their mirroring, prose width and wide blocks, column gap, header and footer offsets. |
| `styles` | One style per block element. |
| `custom-styles` | Named styles that documents apply with `{.name}`. |
| `inline` | Inline code, links, footnote markers. |
| `lists` | Indent, item spacing, bullets per level, and the marker style. |
| `tables` | Cell padding and rules. |
| `callouts` | Padding, background, and bar of the five callout kinds. |
| `footnotes` | Footnote gap and separator. |
| `margin-notes` | The note column for margin notes and sidenotes. |
| `captions` | Separator between label and caption text. |
| `placement` | Whether figures and tables go to the top or bottom of a page by default. |
| `bibliography` | Hanging indent and entry spacing. |
| `toc` | Level indent and dot leaders. |
| `index` | Columns, indents, and the separator of the index. |
| `matter` | Page numbers of the front, main, and back matter of a book. |
| `chapters` | How level 1 headings open chapters: page breaks, sink, number, ornament, drop capital, and lead-in. |
| `scene-break` | The mark of a thematic break. |
| `typesetting` | Rules for page and line breaking: lines under a heading, hyphens in a row, a chapter's last page, hyphen fragments, and no-break spaces. |
| `title-block` | Title slots at the start of the body, and the space below them. |
| `title-page` | Slot groups for a separate title page. |
| `pages` | Header and footer slot groups per page variant. |
| `watermark` | Text turned behind the content of each page, such as "DRAFT". |
| `highlight` | Token colors of code blocks with a language. |
| `labels` | Generated text per language. |

`document` values are defaults. Front matter and `--set` can override each of them. `document.lang` must be `en` or `de`, `citation-style` is `author-date` or `numeric`, and both depths are integers from 1 to 6.

## Tokens

Tokens are named values in four groups: `fonts`, `sizes`, `spacing`, and `colors`. A field uses a token with `$group.name`, for example `"$colors.accent"`.

- The group must match the field. A size field accepts `$sizes.x`, never `$spacing.x`.
- A token may reference another token of its own group. `"caption": "$sizes.small"` is valid.
- Undefined references and cycles are errors.
- Token names use ASCII letters, digits, and hyphens.

Value formats:

| Group | Literal |
| --- | --- |
| `fonts` | A family name defined in `fonts`, or an installed family. |
| `sizes` | A length. |
| `spacing` | A length. |
| `colors` | `#rrggbb`. |

The default theme defines tokens such as `$fonts.body`, `$fonts.heading`, `$fonts.mono`, `$sizes.body`, and `$spacing.margin-side`. The front matter keys `fonts.body`, `fonts.heading`, `fonts.mono` write the three font tokens. A theme should reference those tokens for the setting to take effect.

## Measurements

Lengths are strings with a unit: `pt`, `mm`, `cm`, `in`, or `em`. Examples: `"10.5pt"`, `"28mm"`, `"1.5em"`. Negative values are errors. There is no `px`, no percentages, no viewport units, and no `calc`.

`line-height` is a separate unitless number from 0.8 to 3, a multiple of the element's font size.

`em` needs a font size to resolve against. The basis depends on where the length is used:

| Where | em refers to |
| --- | --- |
| `styles.<name>.size` | The body font size. The body size itself must be absolute and greater than zero. |
| Other lengths in `styles.<name>` (spacing, indents) | That element's own font size. |
| `page.*` (size, margins, column gap and change spacing, offsets) | The body font size. |
| `lists.*` | The `styles.list` size. |
| `tables.*` | The `styles.table-cell` size. |
| `footnotes.*` | The `styles.footnote` size. |
| `margin-notes.*` | The size of the style `margin-notes.style` names. |
| `bibliography.*` | The `styles.bibliography` size. |
| `toc.*` | The `styles.toc-entry` size. |
| `index.*` | The `styles.index-entry` size. |
| `title-block.space-after` | The body font size. |
| Title slot `width` and `space-before` | The size of the slot's style. |
| `inline.*` | The surrounding text size. This is resolved during layout. |

Font sizes must be greater than zero.

## Fonts and images

`fonts` maps a family name to its faces. A face is named by its weight and style:

```json
"fonts": {
  "Source Serif": {
    "regular": "fonts/SourceSerif-Regular.otf",
    "italic": "fonts/SourceSerif-Italic.otf",
    "bold": "fonts/SourceSerif-Bold.otf",
    "bold-italic": null,
    "300": "fonts/SourceSerif-Light.otf",
    "300-italic": "fonts/SourceSerif-LightItalic.otf",
    "600": { "file": "fonts/SourceSerif.ttc", "index": 2 }
  }
}
```

- `regular`, `italic`, `bold`, and `bold-italic` are weight 400 and 700, upright and italic. Other weights from 100 to 900 in steps of 100 are written as the number, with `-italic` for the italic face. Write `regular` and `bold` rather than `400` and `700`, so every face has one name and a later layer replaces it.
- A face is a file path, or `{ "file": ..., "index": n }` for the face at index `n`, counting from 0, of a collection (`.ttc`, `.otc`).
- Only `regular` is required. A style that requests a face its family lacks is an error.

Variable fonts can supply multiple faces from one file:

```json
"fonts": {
  "Public Sans": {
    "regular": { "file": "fonts/PublicSans.ttf", "variable": true },
    "italic": { "file": "fonts/PublicSans-Italic.ttf", "variable": true }
  }
}
```

`variable: true` supplies weights from 100 to 900 in steps of 100 within the file's `wght` axis range. The existing style `weight` selects the instance. A file with an `ital` axis supplies upright and italic instances at 0 and 1 when those values are supported; otherwise its face entry determines the style, so a separate italic file belongs under `italic`. For collections, add `index`; it defaults to 0 for variable files. A file marked variable must contain variation axes. Explicit face entries, including `null`, override generated instances. A requested block face outside the supported range is an error; inline emphasis and strong text keep the usual nearest-face fallback. Other axes, including `opsz`, `wdth`, and `slnt`, stay at the font's defaults.

Installed variable fonts are detected automatically and supply the same weight and italic instances.

A style asks for a face with `weight` and `style`, for example `"weight": 500, "style": "italic"` for `500-italic`. Inline code uses the regular face.

A family name that `fonts` does not define is looked up among the fonts installed on the machine, by its family name, ignoring case. This includes faces in font collections. A family defined in `fonts`, also through front matter `font-files`, wins over an installed one with the same name. Every face a style requests must be installed; a missing family, weight, or style is an error naming what was searched for, and no PDF is written. Installed faces with a condensed or expanded width are used only if the family has no normal width. Druck scans the installed fonts only when a style names such a family, so documents using the bundled and file fonts never depend on the machine.

A font whose license (the OS/2 `fsType` field) restricts embedding or subsetting is still embedded, with a warning that names its file. The license is yours to check.

Inline emphasis and strong text may ask for a face the family lacks. Then the closest face is used: the nearest weight first, then the requested style. For weights from 400 to 500, the weights up to 500 are tried first, then lighter ones, then heavier ones; below 400 lighter ones come first, above 500 heavier ones, as in CSS. So bold-italic falls back to bold, then italic, and italic falls back to regular. Strong text sets weight 700, or keeps a heavier weight of its style. The default fonts are Libertinus Serif and Libertinus Mono, compiled into the binary, so rendering with the default theme never depends on installed fonts.

`images` maps a name to a file path. Title slots refer to images by that name. Only the images that the title layout in use shows are read, so an unused image is not decoded. Images in the document body are not part of the theme; they resolve relative to the document.

Paths must be local. Remote URLs (anything containing `://`) are rejected, and empty paths are errors. Referenced files must exist; `druck check` reports missing ones.

## Block styles

Each entry of `styles` has the same fields, all inherited from the default when omitted:

| Field | Values |
| --- | --- |
| `font` | Font name or `$fonts.x`. |
| `size` | Length or `$sizes.x`. |
| `weight` | 100 to 900 in steps of 100, or `regular` (400) and `bold` (700). |
| `style` | `normal`, `italic`. |
| `color` | `#rrggbb` or `$colors.x`. |
| `line-height` | 0.8 to 3. |
| `align` | `justify`, `left`, `center`, `right`. |
| `hyphenate` | `true`, `false`. |
| `space-before`, `space-after`, `indent`, `first-line-indent` | Length or `$spacing.x`. |
| `tracking` | Letter spacing in em of the style's size, from -0.2 to 1. Default `0`. |
| `uppercase` | `true` sets the text in capitals. Default `false`. |
| `keep-with-next` | `true` keeps the block on the page or in the column of the next block. Default `false`. |
| `figures` | `style`: `lining`, `oldstyle`, or `default`; `spacing`: `proportional`, `tabular`, or `default`. Both default to `default`, the font's own figures. |
| `number-gap` | Only on `heading-1` to `heading-6`. The space between an automatic heading number and the text. `null`, as in the default theme, sets one space after the number. |

`tracking` adds space after every character, spaces included. Tracked text sets no ligatures, so "fi" stays two spaced letters. `uppercase` follows Unicode case mapping, so "Straße" becomes "STRASSE"; the PDF text is the capitals. Inline code keeps its case and spacing. Both apply wherever the style sets text: blocks, title slots, header and footer slots, table cells, and contents entries. List markers and contents page numbers ignore them.

A block with `keep-with-next` never ends a page or column without the next block, as a heading never does. Headings always keep with what follows. A table row in such a style stays with the next row.

With `number-gap` on a heading style, the automatic number is set hanging and the heading text starts the gap after it on every line. Table of contents entries, bookmarks, and running headers still show "1.2 Title". Custom styles based on the heading style inherit the gap.

`align: justify` chooses line breaks for the whole paragraph, stretches or shrinks interword spaces to fill each line except the last, and lets punctuation at the line edges hang slightly into the margins. The other alignments keep natural spaces and also choose breaks for the whole paragraph, so ragged lines come out even. `hyphenate: true` hyphenates words in the document language; code is never hyphenated. Details are in [decisions](DECISIONS.md#2026-10-06-milestone-03-paragraph-composition).

`indent` narrows the block on both sides, as for a quotation. `first-line-indent` applies to a paragraph that follows another paragraph. Vertical spacing between blocks collapses: the larger of one block's `space-after` and the next block's `space-before` applies, and space at the top of a page is dropped. Lines are `size × line-height` apart.

Style names: `body`, `heading-1` to `heading-6`, `title`, `subtitle`, `author`, `date`, `abstract-heading`, `abstract`, `quote`, `list`, `code-block`, `caption`, `table-cell`, `table-header`, `footnote`, `bibliography`, `toc-heading`, `toc-entry`, `header`, `footer`, `watermark`, `scene-break`, `index-letter`, `index-entry`, `index-sub-entry`. The `watermark` style uses only `font`, `size`, `weight`, `style`, `color`, `tracking`, and `uppercase`.

Space between blocks (`space-before` and `space-after`) may grow by up to half its natural height so that page bottoms line up. Inside two columns, the same bound lets the shorter column's spaces grow so that both columns end level. Lines within a block never move apart.

`figures` turns on the OpenType features `lnum`, `onum`, `pnum`, and `tnum`. Oldstyle figures suit running text; tabular lining figures line up in table columns:

```json
"styles": {
  "body": { "figures": { "style": "oldstyle" } },
  "table-cell": { "figures": { "style": "lining", "spacing": "tabular" } }
}
```

A font without a feature keeps its default figures, without a warning. The figures apply wherever the style sets text, also to automatic heading numbers, list numbers, contents page numbers, and numeric bibliography labels. Inline code keeps the code font's default figures. A custom style's `figures` replaces its base style's as a whole.

## Inline code

`inline.code` styles code spans in running text, such as `` `git blame` ``:

| Field | Values |
| --- | --- |
| `font`, `size`, `color` | The code font, its size relative to the surrounding text, and its color. Code uses the regular face. |
| `background` | `#rrggbb` or `$colors.x` for a box behind the code, or `null`, the default, for none. |
| `padding` | Space before and after the code, inside its box, in em of the surrounding text. Default `0pt`. |

The box spans the code from its ascender to its descender, spaces inside the span included. Code that wraps gets a box on each line, padded only at the span's own start and end. The padding counts in the line width, so it applies with or without a background. Code blocks use `styles.code-block` instead.

```json
"inline": {
  "code": { "color": "#24292f", "background": "#eff1f3", "padding": "0.2em" }
}
```

## Callouts

A [callout](AUTHORING.md#callouts) is set in two block styles and a `callouts` section. `styles.callout` sets the text and the space around the box, and `indent` narrows the box on both sides. `styles.callout-label` sets the label above the text; its `space-after` is the gap between label and text. The default theme sets the label in bold small capitals, tracked, and the text upright.

| Key | Effect |
| --- | --- |
| `callouts.padding` | Space between the box edge and the text on all sides, in em of the `callout` style. |
| `callouts.note`, `tip`, `important`, `warning`, `caution` | One entry per kind with `background` and `rule`. |
| `background` | `#rrggbb` or `$colors.x` for the box, or `null` for none. |
| `rule` | `{ "thickness": ..., "color": ... }` for a bar along the left edge of the box, or `null` for none. The text starts after the bar and the padding. |

The label of each kind is in `labels`, see [labels](#labels). An empty label leaves the label line out.

The default theme prints well in grayscale: all kinds sit on a light gray, and the bar grows from a medium gray at 2pt for notes and tips, to dark gray at 3pt for important and warnings, to the text color at 4pt for cautions.

```json
"callouts": {
  "padding": "0.8em",
  "tip": { "background": null, "rule": { "thickness": "1pt", "color": "$colors.accent" } }
}
```

Lines of a callout keep the usual rules against single lines at the top and bottom of a page, and a callout of fewer than four lines is not split. A split callout draws its background and bar on both pages, reaching its padding into the margin where it continues. Inside a footnote a callout is set like a quotation, without box or label.

## Code highlighting

`highlight` colors the tokens of fenced code blocks that name a language. It is `null` to turn highlighting off, or an object with a color for each category, as `#rrggbb` or `$colors.x`:

| Category | Covers |
| --- | --- |
| `comment` | Comments, including doc comments. |
| `keyword` | Keywords and storage words such as `fn`, `let`, `if`, `const`, `export`. |
| `string` | Strings and characters with their quotes and escapes. |
| `number` | Numeric literals. |
| `constant` | Named constants such as `true`, `null`, `None`, and CSS values. |
| `type` | Type, class, and trait names and built-in types. |
| `function` | Names of functions, methods, and macros, and commands in shell code. |
| `operator` | Operators such as `+`, `=>`, `&&`. |
| `punctuation` | Brackets, commas, semicolons, and other delimiters. |
| `tag` | Markup tags, and the keys of YAML and TOML. |
| `attribute` | Attribute names, CSS properties, and annotations. |

Anything else in the code, such as identifiers, keeps the `code-block` color, and so does a category the object leaves out. An unknown category is an error. Layers merge the object by category, so a theme can change one color. `"highlight": null` replaces the whole object, and a later layer that wants it back must list every category it still wants colored.

Only the color changes. The code keeps the `code-block` font, weight, and style, so line widths and page breaks are the same with and without highlighting, and a line that is too wide is still an error. Inline code is never highlighted. A fence without a language, or with one the highlighter does not know, is set in the `code-block` color.

The bundled default colors keywords and tags with the accent color, comments with the muted color, and strings, numbers, types, and functions with dark green, brown, teal, and purple. Operators and punctuation keep the text color. All are dark enough to stay readable in grayscale, where comments are the lightest.

```json
"highlight": {
  "comment": "$colors.muted",
  "keyword": "#1f3d6e",
  "string": "#2d6a4f"
}
```

An EPUB colors the same tokens with `hl-` classes in its stylesheet.

## Custom styles

The `space-before` and `space-after` of the `list` style apply to a list that is not inside another list, and collapse with the space of neighbouring blocks to the larger value. The default theme sets both to `$spacing.block`, as for quotations and code. Items of a nested list follow their parent item at `lists.item-spacing`.

`custom-styles` names styles that documents apply to headings, paragraphs, lists, and table rows and cells with `{.name}`, see [authoring](AUTHORING.md#custom-styles). Title and band slots may name them too. Each is based on another style and lists only what it changes:

```json
"custom-styles": {
  "eyebrow": {
    "based-on": "body",
    "size": "$sizes.small",
    "weight": 500,
    "uppercase": true,
    "tracking": 0.12,
    "keep-with-next": true,
    "first-line-indent": "0pt"
  },
  "checks": { "based-on": "list", "bullets": ["✔"] },
  "step": { "based-on": "heading-3", "number-gap": "0.6em" }
}
```

- `based-on` names a built-in style from `styles` or another custom style. The fields given replace those of the base; the others come from it. A chain of custom styles is followed to its built-in style. An unknown base is an error, and so are custom styles based on each other in a cycle.
- The built-in style at the end of the chain decides what the style applies to: a heading style makes it a heading style, `list` a list style, and any other a paragraph style. Table rows and cells take paragraph styles. Applying a style to another kind of block is an error at the attribute.
- Names use ASCII letters, digits, `-`, and `_`, and cannot be the name of a built-in style.
- `bullets` is allowed on list styles. It replaces `lists.bullets` for a list in that style, one marker per nesting level as there. Numbered lists keep their numbers.
- `marker` is allowed on list styles, and `lists.marker` sets it for all lists. It names a built-in or custom style, such as `"footnote"`, whose font, size, weight, and color set the bullets and numbers. Markers stay on the baseline of their item's first line and still end half an em of the list style before the text. Without a marker style they use the list style; a custom list style without one takes `lists.marker`.
- `rule-below` is allowed on paragraph styles: `none`, `header`, or `row`. A table row in that style draws no rule below it, or the header or row rule instead of its own. Cells and paragraphs in the style ignore it.
- `number-gap` is allowed on heading styles. A heading in that style that starts with a number its author typed, such as "2." followed by a space, sets the number in front and the text after the gap, so every line of the heading starts at the same place. Such a heading is not numbered automatically, and it does not count toward the numbers of other headings.
- Spaces between blocks collapse to the larger, so a heading's `space-before` also separates it from a kept paragraph above it. For a label directly above a heading, give the label the space above and the heading a small `space-before`.

There are no selectors and no cascade: a style applies only where the document names it.

## Footnotes

Footnote text uses `styles.footnote`. Lists, quotations, and code in a note use `styles.list`, custom list styles, `lists.*`, `styles.quote`, and `styles.code-block` scaled by the footnote size over the body size, so a 9 pt note under 11 pt text sets them at 9/11 of their sizes, spacing, and indents. Its `first-line-indent` applies to the second and later paragraphs of a note. The note area sits at the foot of the text area, across the prose width:

| Key | Effect |
| --- | --- |
| `footnotes.gap` | Minimum space between the body text and the separator. |
| `footnotes.separator-width`, `separator-thickness`, `separator-color` | The rule above the notes, starting at the left edge of the prose. |
| `footnotes.spacing` | Space between the separator and the first note, and between notes. |
| `inline.footnote-marker.size`, `raise` | Size and baseline shift of the note number, in the text and at the start of the note, relative to the surrounding text. |

A note that continues on the next page starts there with its number and the `continued` label (see [labels](#labels)).

## Margin notes

`margin-notes` sets the column that holds [margin notes](AUTHORING.md#margin-notes), and footnotes when they are sidenotes:

| Key | Effect |
| --- | --- |
| `margin-notes.width` | The width of the column. |
| `margin-notes.gap` | The space between the outer edge of the prose and the column. |
| `margin-notes.spacing` | The least space between two notes stacked in the column. |
| `margin-notes.style` | The built-in or custom style of the notes. Default `footnote`. |
| `margin-notes.align` | The alignment of note text, instead of the style's: `left`, `right`, `center`, or `justify`. Default `left`, since narrow columns justify poorly. |
| `margin-notes.sidenotes` | `true` sets numbered footnotes in the column instead of at the foot of the page. Default `false`. |

The column is measured from the outer edge of the prose: on the right of odd pages and on the left of even pages, or always on the right with `page.margins.mirror: false`. With `page.text-width` narrower than the frame, the column falls into the free part of the frame, which gives a layout in the style of Tufte's books:

```json
"page": { "margins": { "inner": "22mm", "outer": "16mm" }, "text-width": "112mm", "wide": [] },
"margin-notes": { "width": "46mm", "gap": "6mm", "sidenotes": true }
```

Two configurations are errors, reported only for documents that have margin notes, or footnotes with `sidenotes`: a column that runs off the page, and a column that reaches into the frame while `page.wide` lists block kinds, since wide blocks would run under the notes. The default theme sets a 20mm column 3mm from the prose, which fits the default margins. Sidenotes start with their raised number, as footnotes do. Lists, quotations, and code in notes are scaled like those in footnotes.

## Placement

`placement.figure` and `placement.table` are `here`, `top`, or `bottom`, the placement of figures and tables without a `placement` attribute, see [authoring](AUTHORING.md#placement). Both are `here` in the default theme. The default applies only where placement is allowed, in the main text and in `full-width` blocks of column sections, and not to tables with notes.

A placed block is spaced from the text by the caption style's `space-after`, as figures and tables are in the text.

## Captions

Captions use `styles.caption`. A caption reads "Figure 1: " and then the image description: the `figure` label of the document language, the number, and `captions.separator`. The caption is set across the width of the image's frame, the text area or a column, with the style's alignment. The caption style's `space-before` separates the image from its caption, and its `space-after` is the space above and below the whole figure. The default theme sets captions small, left aligned, and in the muted color.

Images are centered in their frame. Their size and placement have no theme settings; see [authoring](AUTHORING.md#images-and-captions).

Table captions read "Table 1: " and then the caption text, with the `table` label. They are set above the table, across the table's own width from its left edge, and spaced like figure captions. Over a table narrower than a third of its frame the caption runs on to the frame's right edge instead.

## Tables

Header cells use `styles.table-header` and body cells `styles.table-cell`. A row's custom style replaces them for its cells, and a cell's own custom style replaces the row's. A column aligned in the Markdown, or with the `align` attribute of a list table, overrides the style's `align` for the cells of that column that have no style of their own; other columns keep it. `hyphenate` decides whether long words in cells may break, which also lowers the narrowest width a column can take. In a cell the style's `indent` narrows the text, and spacing applies between the paragraphs and lists of a list table cell, but not above the first or below the last. Lists in cells use the `list` style or their own custom style.

| Key | Effect |
| --- | --- |
| `tables.cell-padding` | Space on every side between a cell's text and its edges. |
| `tables.top-rule` | The rule above the header row: `{ "thickness": ..., "color": ... }`, or `null` for none. |
| `tables.header-rule` | The rule below the header row, or `null`. |
| `tables.row-rule` | The rule below each body row, or `null`. |

Rule thicknesses use the table cell size for `em`. The default theme draws all three as hairlines in the `rule` color. A row style's `rule-below` replaces the rule below that row, see [custom styles](#custom-styles); totals rows without rules use `"rule-below": "none"`.

Columns take their natural width when the table fits, else they share the width as described in [authoring](AUTHORING.md#tables). A table narrower than its frame is centered in it. A table listed in `page.wide` that fits in the prose width is centered there; a wider one starts where the prose starts and takes the width it needs, up to the frame.

There are no vertical rules. A table is spaced like a figure: the caption style's `space-after` above and below it, and its `space-before` between the caption and the table. The header row, with its rules, repeats at the top of each page or column a table continues in.

## Margins and text width

The margins leave the margin frame, the text area of the page. `page.margins.inner` is the left margin of odd pages, counted from 1 including the title page. With `mirror: true`, the default, it is the right margin of even pages, as in a bound book. With `mirror: false` the inner margin is on the left of every page, for the body, the bands, and the title page.

`page.text-width` narrows prose to a column measured from the inner edge of the frame; `null`, the default, uses the whole frame. Headings, paragraphs, lists, quotations, footnotes, the title block, the table of contents, the bibliography, and column sections use the prose width. The block kinds listed in `page.wide` span the whole frame instead: `table`, `figure`, and `code-block`, all three by default. A wide block widens only where it stands directly in the prose, also inside `keep` and in a `full-width` block of a column section. Inside lists, quotations, columns, and footnotes it keeps the width it is in. Headers, footers, and title page groups always use the frame.

With mirrored margins the inner edge is on the right of even pages, so prose moves to the right there while wide blocks keep spanning the frame. A narrower table is centered in the frame, as it is without a text width.

The text width must be greater than zero and at most the frame width.

## Columns

`page.column-gap` is the space between the two columns of a `columns` section. Each column is half of what remains of the prose width. `page.column-change-spacing` is the least space between a column section and the full-width blocks above and below it; a larger block spacing wins. Its default, `null`, uses the column gap. Block styles, indents, and list markers apply inside a column as they do at full width, measured within the column. Footnotes stay across the prose width.

Other sections have their own fields; the [schema](../schema/theme.v1.schema.json) lists them all.

## Templates

Title slots, header/footer slots, and the watermark hold text with placeholders. Druck fills them from the document's metadata, its text, the date, and the page.

### Placeholders

| Kind | Placeholders |
| --- | --- |
| Metadata | `{title}`, `{subtitle}`, `{author}`, `{date}`, `{abstract}`, `{meta.key}` |
| Pages | `{section}`, `{subsection}`, `{page}`, `{pages}`, `{section-page}`, `{section-pages}` |
| Statistics | `{chars}`, `{chars-no-spaces}`, `{words}`, `{sentences}`, `{paragraphs}`, `{reading-time}`, `{figures}`, `{tables}` |
| Other | `{build-date}`, `{year}`, `{draft}` |

- Write `{{` and `}}` for literal braces.
- Unknown placeholders and unbalanced braces are errors.
- Values are inserted as text. They are never read as layout commands or expressions.
- Title slots may not use the page placeholders.
- Headers, footers, and the watermark may not use `{abstract}`.
- A slot's value is missing when any placeholder in it has no value or only spaces.
- `{author}` joins several authors with commas. `{page}` is the page number, counted from 1 including the title page, and `{pages}` the number of pages. `{section}`, `{subsection}`, `{section-page}`, and `{section-pages}` are explained under [header and footer bands](#header-and-footer-bands).
- `{meta.key}` is the entry `key` of the front matter `meta` map. Keys use letters, digits, `-`, and `_`. Any key is accepted here, since only the document knows its keys; a key the document lacks is a missing value.
- The statistics count the document's own text, as described in [placeholders](AUTHORING.md#placeholders). Counts are grouped by thousands for the document language, as in "12,480" and "12.480".
- `{build-date}` is the day of the run, as in "7 October 2026" or "7. Oktober 2026", and `{year}` its year.
- `{draft}` is the `draft` label when `document.draft` is true, and missing otherwise. A slot such as `"{draft} · {build-date}"` therefore appears only in drafts.

Documents can use the placeholders that do not depend on the page in their text, see [placeholders](AUTHORING.md#placeholders).

A line break in slot text starts a new line, so `"Studio\nMain Street 1"` sets two lines. A `meta` value that is a list starts a new line with each entry after the first; text before the placeholder joins the first entry and text after it the last. Empty lines are dropped. Within a value, line breaks follow the rules of the slot kind below.

### Title slots

`title-block` has an ordered `slots` array, and each group of `title-page` has one. A slot is either text or an image:

| Field | Meaning |
| --- | --- |
| `text` | Slot text with placeholders. Use this or `image`, not both. |
| `image` | A name from the theme `images` map. |
| `width` | Image width. Required for image slots, an error on text slots. It may not exceed the prose width in the title block, or the group width on the title page. |
| `style` | One of `title`, `subtitle`, `author`, `date`, `abstract-heading`, `abstract`, `body`, or the name of a custom style. Default `body`. |
| `required` | Default `false`. |
| `space-before` | Default `0pt`. May not exceed the text height. |

Because arrays replace whole, a theme that changes one slot restates the whole list.

The `title-page` groups fill the first page when a document sets `title-page: true`, see [slot groups](#slot-groups). Otherwise the `title-block` slots start the body, provided the document has a value for at least one of their placeholders; a document without metadata has no title block. `title-block.space-after` is the space between the block and the body.

Slots are set in order, each `space-before` below the previous one. The style of a slot sets its font, alignment, and indent; its own `space-before` and `space-after` are not used. An optional slot whose value is missing is left out with its `space-before`. A slot in the `abstract` style starts with the `abstract` label in the `abstract-heading` style, followed by that style's `space-after`. Each line of slot text wraps at the available width. In values, blank lines start a new paragraph, which gets the style's `first-line-indent`, and other line breaks are spaces. An image slot is aligned like text in its style.

### Slot groups

The title page and each header and footer are arrays of slot groups. A group stacks its slots in a box placed on the margin frame:

| Field | Meaning |
| --- | --- |
| `anchor` | `top-left`, `top-right`, `middle-left`, `middle-right`, `bottom-left`, or `bottom-right`. Required. |
| `offset` | `{ "x": ..., "y": ... }`, each default `0pt`. `x` moves the group in from the anchored left or right edge. `y` moves it down from a top or middle anchor and up from a bottom one. |
| `width` | Default: the frame width less `offset.x`. The group must fit in the frame with its offset. |
| `align` | `left`, `center`, `right`, or `justify` for every line in the group. Default: each slot's style alignment. |
| `slots` | Title slots on the title page, header and footer slots in bands. |

Lengths in `offset` and `width` use the body size for `em`. Because margins position the frame, a group moves with them, and with `mirror: true` a band's left and right anchors follow the margins on even pages.

On the title page an anchor places the group's box, from the space above its first slot to the bottom of its last line: its top edge `y` below the frame's top, its middle `y` below the frame's middle, or its bottom edge `y` above the frame's bottom. A group that runs past the top or bottom of the frame is an error naming the group. The default theme has one group at `top-left` that stacks its slots from the top of the text area.

### Header and footer bands

Each page variant has a `header` and a `footer`: an array of slot groups, or `null` for none. A header or footer slot has these fields:

| Field | Meaning |
| --- | --- |
| `text` | Slot text with placeholders. Required. |
| `style` | One of the title slot styles or a custom style. Default: the `header` or `footer` style. Tracking and capitals apply, so a custom style based on `footer` can set spaced capitals. |
| `required` | Default `false`. |
| `space-before` | Space above the slot's first line, default `0pt`. `em` refers to the slot style size. |

The header's baseline is `page.header-offset` above the text area and the footer's `page.footer-offset` below it. Groups are anchored to that baseline: a top anchor puts the group's first baseline `y` below it, a bottom anchor its last baseline `y` above it, and a middle anchor centers the group's baselines `y` below it. So a multi-line footer anchored at the top grows down toward the page edge, and a header anchored at the bottom grows up. Lines follow each other at the style's line height plus each slot's `space-before`. Every line of slot text is one line: it never wraps, and line breaks in values are spaces. A line wider than its group and lines of two groups that overlap are errors naming the band or group and the page, since nothing is clipped. An optional slot without a value is left out.

The default theme's left, center, and right band slots are groups as wide as the frame, aligned left, center, or right. Groups do not swap on even pages, so use the `odd` and `even` variants for bands that change sides.

`{section}` is the first level 1 heading that starts on the page, otherwise the last level 1 heading on an earlier page. `{subsection}` is the first level 2 heading on the page after that section, otherwise the last level 2 heading before the page, unless a level 1 heading followed it. Both include the heading number. On pages before the first heading they have no value.

`{section-page}` counts the consecutive pages that show the same `{section}`, from 1, and `{section-pages}` is their number, so `"{section-page} of {section-pages}"` reads "2 of 5". Blank pages count. Both have no value where `{section}` has none.

### Missing values

A slot marked `required` whose value is missing is an error. Title slots are checked for the title layout in use as soon as the document's metadata is known, by `druck check` and `druck render`. Header and footer slots need the page, so `druck render` checks them after layout and names the page. Optional slots with no value are omitted along with their spacing.

## Page variants

`pages` has the variants `title`, `first`, `opening`, `odd`, `even`, and `body`. Only `body` is required. Which variant applies to a page:

| Page | Order tried |
| --- | --- |
| Title page, and in a book with parts the pages before the first part | `title`, then `body`. |
| First body page | `opening` if a level 1 heading starts on it, `first`, then `odd` or `even` by parity, then `body`. |
| Other body pages | `opening` if a level 1 heading starts on it, then `odd` or `even` by parity, then `body`. |

A null variant is skipped. Parity follows the physical page index in the PDF, counted from 1 and including the title page and blank pages. Without [parts](#books) the displayed page number `{page}` is the same number, so odd numbers are on odd pages, the right-hand pages in duplex printing. The first body page is the first page without a title page and the second page with one, or the third with `document.duplex`.

With `document.duplex: true` the body starts on an odd page: a blank page follows the title page, and the table of contents ends its page and is followed by a blank page if the text would otherwise start on an even one. Blank pages have no header or footer but count in `{page}` and `{pages}`.

The inner margin sits on the left of odd pages and, unless `page.margins.mirror` is `false`, on the right of even pages.

## Watermark

`watermark` sets one line of text large and turned behind the content of every page that has bands, so not on blank pages. It is `null` for none, or:

| Field | Meaning |
| --- | --- |
| `text` | Text with placeholders, set in the `watermark` style. Line breaks are spaces. |
| `angle` | The counterclockwise turn in degrees, from -180 to 180. |

The center of the line, halfway between its baseline and the height of capitals, lies on the center of the page. The line is set at the style's size, or smaller so that its turned box fits within 90% of the page width and height. While a placeholder in the text has no value, there is no watermark.

The default theme follows LaTeX's `draftwatermark` package: `{draft}` at 45 degrees in light gray capitals at 160pt, so only drafts show it, as "DRAFT" or "ENTWURF". A fixed text such as `"Confidential"` shows on every page.

## Labels

`labels` has an `en` and a `de` set with the keys `figure`, `table`, `section`, `page`, `contents`, `abstract`, `references`, `continued`, `draft`, `chapter`, and the callout labels `note`, `tip`, `important`, `warning`, `caution`. The set for `document.lang` is used.

`labels` has an `en` and a `de` set with the keys `figure`, `table`, `section`, `page`, `contents`, `abstract`, `references`, `continued`, `draft`, `chapter`, `index`, `see`, `see-also`, `symbols`. The set for `document.lang` is used.

| Key | Used for |
| --- | --- |
| `figure`, `table` | Captions, as in "Figure 1: ", and references to figures and tables, as in "Figure 1". |
| `section` | References to numbered headings, as in "Section 2.1". |
| `page` | Page references, as in "page 7". |
| `contents` | The table of contents heading. |
| `abstract` | The heading above an `abstract` title slot. |
| `references` | The bibliography heading, also in the table of contents, bookmarks, and running headers. |
| `continued` | The continuation of a footnote on the next page. |
| `draft` | The value of `{draft}` in drafts. |
| `chapter` | Before a chapter number above its heading with `chapters.number-label`, as in "Chapter 2". |
| `index` | The index heading, also in the table of contents, bookmarks, and running headers. |
| `see`, `see-also` | Before the entries an index entry refers to, as in "see Line breaking". |
| `symbols` | Above the index entries that start with a digit or symbol. |

A reference joins the label and the number with a no-break space. `captions.separator` is the text between the label and the caption, as in "Figure 1: ".

## Table of contents

With `document.toc` the body starts, after the title block, with the `contents` label in the `toc-heading` style and one entry per heading down to `document.toc-depth`, in the `toc-entry` style.

| Key | Effect |
| --- | --- |
| `toc.level-indent` | Indent per heading level below the first. Lines after an entry's first are indented by one level more. |
| `toc.leader` | Whether a row of dots leads from the entry text to its page number. |
| `toc.level-styles` | A list of style names, built-in or custom, for the entries of level 1, 2, and so on. The last one repeats for deeper levels. Absent or empty, every entry uses `toc-entry`. |

The page number is right aligned at the edge of the text area, in a column at least three digits wide. Leader dots sit on a grid shared by all entries, so they line up. Each entry links to its heading.

## Bibliography

A document that cites sets a bibliography: the `references` label as an unnumbered heading in the `heading-1` style, then one entry per cited work in the `bibliography` style. `document.citation-style` chooses `author-date` or `numeric`; front matter can override it.

| Key | Effect |
| --- | --- |
| `bibliography.hanging-indent` | Indent of an entry's lines after its first. |
| `bibliography.entry-spacing` | Minimum space between entries. The style's own `space-before` and `space-after` also apply around each entry, and the larger space wins. |

Links in entries use `inline.link`, and so do citations, which link to their entry. Entry content and ordering are described in [authoring](AUTHORING.md#citations-and-bibliography).

## Index

A document with [index marks](AUTHORING.md#index) sets an index: the `index` label as an unnumbered heading in the `heading-1` style, which opens a chapter as `chapters` sets, then the entries grouped under their first letter.

| Key | Effect |
| --- | --- |
| `index.columns` | `2` sets the entries in two columns below the heading, with the column gap of `page.column-gap`; `1` across the prose width. Inside a `columns` section the index takes the section's columns. Default `2`. |
| `index.level-indent` | Indent of sub-entries per level. Default `1em`. |
| `index.hanging-indent` | Indent of an entry's lines after its first, beyond its level. Default `2em`. |
| `index.separator` | Text between a term and its first page number or reference, as in "Hyphenation, 3". Default `", "`. |

The styles:

| Style | Sets |
| --- | --- |
| `index-letter` | The letter above each group, or the `symbols` label. It stays with the first entry below it. |
| `index-entry` | Entries. |
| `index-sub-entry` | Sub-entries, at both levels. |

An entry stays with its first sub-entry, and its last sub-entry with the one before. Page numbers are links and use `inline.link`. "see" and "see also" are set in italics. Page numbers are separated by a comma, ranges joined by an en dash, and references by a semicolon.

## Books

These sections shape the [parts, chapters, and scene breaks](AUTHORING.md#books) of a book. [`samples/themes/novel.json`](../samples/themes/novel.json) uses all of them.

`matter` has a `front`, `main`, and `back` entry for the parts that `::: front-matter`, `::: main-matter`, and `::: back-matter` start:

| Field | Values |
| --- | --- |
| `page-numbers` | `decimal` (1), `decimal-leading-zero` (01), `lower-roman` (i), or `upper-roman` (I). |
| `restart` | `true` counts the part's pages from 1. `false` continues the count of the part before it. |

The default theme numbers the front matter in lower-case roman numerals and the main matter from 1, and the back matter continues the main matter. `{pages}` is the last number of the count a page belongs to.

`chapters` applies to every level 1 heading of the body, the bibliography's included. `em` lengths refer to the `heading-1` size.

| Field | Values |
| --- | --- |
| `break-before` | `none`, `page` for a new page, or `recto` for a new odd page, after a blank page if needed. Default `none`. |
| `sink` | The distance from the top of the text area to a chapter that starts a page, replacing the heading's `space-before`. Default `0pt`. |
| `number-format` | How the chapter part of heading numbers is written, as in "II" or "II.3", with the values of `page-numbers`. Default `decimal`. |
| `number-position` | `inline` before the heading text, or `above` on a line of its own. Default `inline`. |
| `number-style` | The built-in or custom style of a number above the heading. It takes the heading's `space-before`; its own `space-after` separates it from the heading text. Default `heading-1`. |
| `number-label` | `true` puts the `chapter` label before a number above the heading. Default `false`. |
| `ornament` | `null`, or an image centered below the heading: `{"image": name, "width": length, "space-before": length}`. The heading's `space-after` goes below the image. |
| `drop-cap` | The lines a drop capital spans in a chapter's first paragraph, 2 to 5, or 0 for none. Default `0`. |
| `lead-in` | The number of words at the start of a chapter's first paragraph set in small capitals. Default `0`. |

A drop capital is the paragraph's first letter, with any punctuation before it, in the paragraph's font and weight, upright and in its color. It reaches from the cap height of the first line to the baseline of the last line it spans, and those lines start a quarter em after it. The lead-in uses the font's own small capitals, the OpenType `smcp` feature, and capitals at 80% size if the font has none. Libertinus has them.

`scene-break` sets the mark of a thematic break:

| Field | Values |
| --- | --- |
| `text` | The mark, set as one line in the `scene-break` style. Default `* * *`. |
| `image`, `width` | A name from `images` and its width, set instead of the text and aligned like the style. Default `null`. |
| `blank` | `true` makes the break an empty line as high as the mark, which shows the mark only where a break starts a page. Default `false`. |

The `scene-break` style's spacing goes around the mark. The default centers it at body size with block spacing.

## Typesetting

`typesetting` tunes the rules that page and line breaking follow. The built-in rules need no setting: a page avoids starting or ending with a one-line paragraph, and in a `duplex` document a right-hand page ends at the height of the left-hand page it faces. Each rule is a cost weighed against the others, so one may still be broken where keeping it would cost more, such as a page that would run several lines short. `druck check` lists the rules broken, see [typesetting warnings](AUTHORING.md#typesetting-warnings).

| Field | Values |
| --- | --- |
| `heading-lines` | The lines a heading at the foot of a page wants below it, 2 to 10. Each line fewer costs as much as a page one line short. Fewer than 2 never happen. Default `3`. |
| `hyphens-in-a-row` | The most hyphenated lines in a row, 1 to 10. A paragraph breaks it only if no other breaking exists. Default `2`. |
| `chapter-last-lines` | The fewest lines the last page of a chapter wants, before a page break or the end, 1 to 30. A chapter of one page is exempt. Default `5`. |
| `hyphen-fragments` | `[before, after]`, the fewest letters before and after a hyphenation point, each 1 to 8, for every language. `null` uses the language's own: 2 and 3 in English, 3 and 3 in German. Default `null`. |
| `no-break-spaces` | `true` joins abbreviations such as "z. B.", a number and its unit, and "S.", "Nr.", "p.", or "§" and their number with no-break spaces, see [no-break spaces](AUTHORING.md#no-break-spaces). Default `true`. |

German at 3 letters leaves fewer short fragments such as "ab-hängt" but strains the spacing of a narrow column more. Where `druck check` reports many loose lines, `"hyphen-fragments": [2, 2]` is what LaTeX uses for German.

## EPUB

An [EPUB](AUTHORING.md#epub) takes a stylesheet derived from the theme. Readers reflow the text and let their users change the size, so the mapping keeps proportions rather than positions.

What carries over:

- **Block styles** become CSS for the elements they style: `body` for paragraphs, `heading-1` to `heading-6`, `quote`, `callout`, `callout-label`, `list`, `code-block`, `caption`, `table-header`, `table-cell`, `footnote`, `bibliography`, `toc-heading`, `toc-entry`, and `scene-break`. Each sets the font, weight, style, color, line height, alignment, hyphenation, tracking as `letter-spacing`, capitals as `text-transform`, and figures as `font-variant-numeric`.

- **Block styles** become CSS for the elements they style: `body` for paragraphs, `heading-1` to `heading-6`, `quote`, `list`, `code-block`, `caption`, `table-header`, `table-cell`, `footnote`, `bibliography`, `toc-heading`, `toc-entry`, `scene-break`, `index-letter`, `index-entry`, and `index-sub-entry`. Each sets the font, weight, style, color, line height, alignment, hyphenation, tracking as `letter-spacing`, capitals as `text-transform`, and figures as `font-variant-numeric`.
- **Sizes** are relative to the `body` size, which is the reader's text size, so a heading at 15pt over a 10pt body is `1.5rem`. Space before and after, indents, and `first-line-indent` are in ems of the style's own size. Only a paragraph that follows another one has a first-line indent, as in the PDF.
- **Custom styles** become classes named `s-` and the style name, such as `.s-epigraph`, on the paragraphs, headings, lists, table rows, and cells that apply them. So do the built-in and custom styles that title slots, `chapters.number-style`, and list markers name. A row style's `rule-below` sets the border below its row.
- **Callouts** become an `aside` with the classes `callout` and the kind. The text style, margins, and padding go on `aside.callout`, the label style on its first paragraph, and each kind's background and bar (as a left border) on `aside.callout.note` and its siblings.
- **Lists** take `lists.indent`, `lists.item-spacing`, and the bullets by depth. Readers without text bullets show discs.
- **Tables** take `tables.cell-padding` and the top, header, and row rules. Column alignment applies as in the PDF.
- **Code highlighting** becomes a class per category, such as `.hl-keyword`, with the category's color on `pre` content.
- **Inline styles** take the code font, size, color, background, and padding, the link color and underline, and the footnote marker's size and raise.
- **Footnotes** follow the chapter below a separator of `footnotes.separator-width`, `separator-thickness`, and `separator-color`, `footnotes.gap` below the text and `footnotes.spacing` apart. Sidenotes are footnotes there.
- **Margin notes** follow the block that references them in the style `margin-notes.style` names, indented by `margin-notes.gap` and `margin-notes.spacing` above and below.
- **Chapters** keep `sink` as the space above a chapter heading, the number above the heading with the `chapter` label, the ornament, the lead-in in small capitals, and the drop capital. The drop capital uses CSS `initial-letter` where readers support it, and a floated letter of about the same size elsewhere.
- **Scene breaks** show the text or image mark. A `blank` break shows its mark too, since a reader's page breaks are unknown.
- **Images** keep their size relative to the text: an image half as wide as the prose width in the PDF is half as wide as the text.

Fonts are embedded when they ship with Druck or the theme or front matter names their files. A variable file is embedded whole and declared for the weight range of its `wght` axis; an `ital` axis is not used, so give italics their own entry. Faces in collections (`.ttc`, `.otc`) are not embedded, since readers cannot load them, and a warning names the file. Installed fonts are never embedded, since their licenses rarely allow passing them on: the stylesheet names the family with a serif fallback, monospace for the code font, and `render` prints a warning. A font whose license restricts embedding is embedded with the same warning as in the PDF.

Left out, because readers lay out the pages: the page size, margins, text width, and `wide`, the title page's anchors and positions, headers, footers, and page variants, `matter` page numbers, `chapters.break-before` (every chapter starts a new file), two-column layout, `index.columns`, the watermark, `toc.leader`, and the scored page breaking. Headings and styles with `keep-with-next` ask readers not to break the page after them. A numeric bibliography uses `bibliography.hanging-indent` instead of a column as wide as its labels.

Left out, because readers lay out the pages: the page size, margins, text width, and `wide`, the note column's width and `align`, `placement`, the title page's anchors and positions, headers, footers, and page variants, `matter` page numbers, `chapters.break-before` (every chapter starts a new file), two-column layout, the watermark, `toc.leader`, and the scored page breaking. Headings and styles with `keep-with-next` ask readers not to break the page after them. A numeric bibliography uses `bibliography.hanging-indent` instead of a column as wide as its labels.

## Resources and distribution

A path resolves relative to the layer that supplied it:

- Theme fonts and images resolve relative to the directory of the theme file.
- Font files from front matter `font-files` resolve relative to the document.
- Paths from `--set` resolve relative to the working directory.
- Bundled resources stay bundled.

Merging never moves a path to another origin. If a document overrides one face of a theme family, only that face changes origin.

To distribute a theme, ship the JSON file together with its asset folder and keep every path relative, for example `fonts/Name.otf` next to `theme.json`. There is no archive format and no registry.

## Example

A partial theme: a sans heading font (the default heading styles use bold and italic faces, so the family needs all four), a wider inner margin, a footer with the title, and German labels changed.

```json
{
  "$schema": "../schema/theme.v1.schema.json",
  "version": 1,
  "fonts": {
    "Inter": {
      "regular": "fonts/Inter-Regular.otf",
      "italic": "fonts/Inter-Italic.otf",
      "bold": "fonts/Inter-Bold.otf",
      "bold-italic": "fonts/Inter-BoldItalic.otf"
    }
  },
  "tokens": {
    "fonts": { "heading": "Inter" },
    "colors": { "accent": "#7a2a2a" }
  },
  "page": {
    "margins": { "inner": "32mm" }
  },
  "styles": {
    "heading-1": { "color": "$colors.accent" }
  },
  "pages": {
    "body": {
      "footer": [
        { "anchor": "top-left", "slots": [{ "text": "{title}" }] },
        { "anchor": "top-right", "width": "30mm", "align": "right", "slots": [{ "text": "{page}|{pages}" }] }
      ]
    }
  },
  "labels": {
    "de": { "figure": "Abb." }
  }
}
```

Every other value comes from the default theme.
