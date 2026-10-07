# Writing themes

A theme is one JSON file plus any local assets (fonts, images) it references. It controls the design of a document: element styles, page geometry, title blocks, headers, footers, and generated labels. Document authors change a small subset of it from front matter, see [authoring](AUTHORING.md).

`kyber check` loads and validates a theme, and `kyber check --print-config` shows the resolved result. `kyber render` uses it to typeset a document.

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
| `fonts.<family>.<face>` other than `regular` | The family has no such face. |

`pages.body` itself cannot be null, and `fonts.<family>.regular` is required.

## Sections

| Key | Purpose |
| --- | --- |
| `version`, `$schema` | Format version and editor hint. |
| `document` | Defaults for document settings: `lang`, `title-page`, `toc`, `numbered-headings`, `numbering-depth`, `toc-depth`, `citation-style`. |
| `fonts` | Font families and their files. |
| `images` | Named images used by title slots. |
| `tokens` | Named fonts, sizes, spacing, and colors. |
| `page` | Page size, margins and their mirroring, prose width and wide blocks, column gap, header and footer offsets. |
| `styles` | One style per block element. |
| `custom-styles` | Named styles that documents apply with `{.name}`. |
| `inline` | Inline code, links, footnote markers. |
| `lists` | Indent, item spacing, bullets per level. |
| `tables` | Cell padding and rules. |
| `footnotes` | Footnote gap and separator. |
| `captions` | Separator between label and caption text. |
| `bibliography` | Hanging indent and entry spacing. |
| `toc` | Level indent and dot leaders. |
| `title-block` | Title slots at the start of the body, and the space below them. |
| `title-page` | Slot groups for a separate title page. |
| `pages` | Header and footer slot groups per page variant. |
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
| `page.*` (size, margins, column gap, offsets) | The body font size. |
| `lists.*` | The `styles.list` size. |
| `tables.*` | The `styles.table-cell` size. |
| `footnotes.*` | The `styles.footnote` size. |
| `bibliography.*` | The `styles.bibliography` size. |
| `toc.*` | The `styles.toc-entry` size. |
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

A style asks for a face with `weight` and `style`, for example `"weight": 500, "style": "italic"` for `500-italic`. Inline code uses the regular face.

A family name that `fonts` does not define is looked up among the fonts installed on the machine, by its family name, ignoring case. This includes faces in font collections. A family defined in `fonts`, also through front matter `font-files`, wins over an installed one with the same name. Every face a style requests must be installed; a missing family, weight, or style is an error naming what was searched for, and no PDF is written. Installed faces with a condensed or expanded width are used only if the family has no normal width. Kyber scans the installed fonts only when a style names such a family, so documents using the bundled and file fonts never depend on the machine.

A font whose license (the OS/2 `fsType` field) restricts embedding or subsetting is still embedded, with a warning that names its file. The license is yours to check.

Inline emphasis and strong text may ask for a face the family lacks. Then the closest face is used: the nearest weight first, then the requested style. For weights from 400 to 500, the weights up to 500 are tried first, then lighter ones, then heavier ones; below 400 lighter ones come first, above 500 heavier ones, as in CSS. So bold-italic falls back to bold, then italic, and italic falls back to regular. Strong text sets weight 700, or keeps a heavier weight of its style. The default fonts are Libertinus Serif and Libertinus Mono, compiled into the binary, so rendering with the default theme never depends on installed fonts.

`images` maps a name to a file path. Title slots refer to images by that name. Images in the document body are not part of the theme; they resolve relative to the document.

Paths must be local. Remote URLs (anything containing `://`) are rejected, and empty paths are errors. Referenced files must exist; `kyber check` reports missing ones.

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

`tracking` adds space after every character, spaces included. Tracked text sets no ligatures, so "fi" stays two spaced letters. `uppercase` follows Unicode case mapping, so "Straße" becomes "STRASSE"; the PDF text is the capitals. Inline code keeps its case and spacing. Both apply wherever the style sets text that can wrap: blocks, title slots, table cells, and contents entries. Header and footer bands, list markers, and contents page numbers ignore them.

A block with `keep-with-next` never ends a page or column without the next block, as a heading never does. Headings always keep with what follows.

`align: justify` chooses line breaks for the whole paragraph, stretches or shrinks interword spaces to fill each line except the last, and lets punctuation at the line edges hang slightly into the margins. The other alignments keep natural spaces and also choose breaks for the whole paragraph, so ragged lines come out even. `hyphenate: true` hyphenates words in the document language; code is never hyphenated. Details are in [decisions](DECISIONS.md#2026-10-06-milestone-03-paragraph-composition).

`indent` narrows the block on both sides, as for a quotation. `first-line-indent` applies to a paragraph that follows another paragraph. Vertical spacing between blocks collapses: the larger of one block's `space-after` and the next block's `space-before` applies, and space at the top of a page is dropped. Lines are `size × line-height` apart.

Style names: `body`, `heading-1` to `heading-6`, `title`, `subtitle`, `author`, `date`, `abstract-heading`, `abstract`, `quote`, `list`, `code-block`, `caption`, `table-cell`, `table-header`, `footnote`, `bibliography`, `toc-heading`, `toc-entry`, `header`, `footer`.

Space between blocks (`space-before` and `space-after`) may grow by up to half its natural height so that page bottoms line up. Inside two columns, the same bound lets the shorter column's spaces grow so that both columns end level. Lines within a block never move apart.

## Custom styles

`custom-styles` names styles that documents apply to headings, paragraphs, and lists with `{.name}`, see [authoring](AUTHORING.md#custom-styles). Each is based on another style and lists only what it changes:

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
- The built-in style at the end of the chain decides what the style applies to: a heading style makes it a heading style, `list` a list style, and any other a paragraph style. Applying a style to another kind of block is an error at the attribute.
- Names use ASCII letters, digits, `-`, and `_`, and cannot be the name of a built-in style.
- `bullets` is allowed on list styles. It replaces `lists.bullets` for a list in that style, one marker per nesting level as there. Numbered lists keep their numbers.
- `number-gap` is allowed on heading styles. A heading in that style that starts with a number its author typed, such as "2." followed by a space, sets the number in front and the text after the gap, so every line of the heading starts at the same place. Such a heading is not numbered automatically, and it does not count toward the numbers of other headings.
- Spaces between blocks collapse to the larger, so a heading's `space-before` also separates it from a kept paragraph above it. For a label directly above a heading, give the label the space above and the heading a small `space-before`.

There are no selectors and no cascade: a style applies only where the document names it.

## Footnotes

Footnote text uses `styles.footnote`. Its `first-line-indent` applies to the second and later paragraphs of a note. The note area sits at the foot of the text area, across the prose width:

| Key | Effect |
| --- | --- |
| `footnotes.gap` | Minimum space between the body text and the separator. |
| `footnotes.separator-width`, `separator-thickness`, `separator-color` | The rule above the notes, starting at the left edge of the prose. |
| `footnotes.spacing` | Space between the separator and the first note, and between notes. |
| `inline.footnote-marker.size`, `raise` | Size and baseline shift of the note number, in the text and at the start of the note, relative to the surrounding text. |

A note that continues on the next page starts there with its number and the `continued` label (see [labels](#labels)).

## Captions

Captions use `styles.caption`. A caption reads "Figure 1: " and then the image description: the `figure` label of the document language, the number, and `captions.separator`. The caption is set across the width of the image's frame, the text area or a column, with the style's alignment. The caption style's `space-before` separates the image from its caption, and its `space-after` is the space above and below the whole figure. The default theme sets captions small, left aligned, and in the muted color.

Images are centered in their frame. Their size and placement have no theme settings; see [authoring](AUTHORING.md#images-and-captions).

Table captions read "Table 1: " and then the caption text, with the `table` label. They are set above the table, across the width of its frame, and spaced like figure captions.

## Tables

Header cells use `styles.table-header` and body cells `styles.table-cell`. A column aligned in the Markdown overrides the style's `align` for that column; other columns keep it. `hyphenate` decides whether long words in cells may break, which also lowers the narrowest width a column can take. The styles' spacing and indent fields do not apply inside cells.

| Key | Effect |
| --- | --- |
| `tables.cell-padding` | Space on every side between a cell's text and its edges. |
| `tables.rule-thickness` | The rules above and below the header row and below each body row. `0pt` draws none. |
| `tables.rule-color` | The color of those rules. |

There are no vertical rules. A table is spaced like a figure: the caption style's `space-after` above and below it, and its `space-before` between the caption and the table. The header row, with its rules, repeats at the top of each page or column a table continues in.

## Margins and text width

The margins leave the margin frame, the text area of the page. `page.margins.inner` is the left margin of odd pages, counted from 1 including the title page. With `mirror: true`, the default, it is the right margin of even pages, as in a bound book. With `mirror: false` the inner margin is on the left of every page, for the body, the bands, and the title page.

`page.text-width` narrows prose to a column measured from the inner edge of the frame; `null`, the default, uses the whole frame. Headings, paragraphs, lists, quotations, footnotes, the title block, the table of contents, the bibliography, and column sections use the prose width. The block kinds listed in `page.wide` span the whole frame instead: `table`, `figure`, and `code-block`, all three by default. A wide block widens only where it stands directly in the prose, also inside `keep` and in a `full-width` block of a column section. Inside lists, quotations, columns, and footnotes it keeps the width it is in. Headers, footers, and title page groups always use the frame.

With mirrored margins the inner edge is on the right of even pages, so prose moves to the right there while wide blocks keep spanning the frame. A narrower table is centered in the frame, as it is without a text width.

The text width must be greater than zero and at most the frame width.

## Columns

`page.column-gap` is the space between the two columns of a `columns` section. Each column is half of what remains of the prose width. The gap is also the least space between a column section and the full-width blocks above and below it; a larger block spacing wins. Block styles, indents, and list markers apply inside a column as they do at full width, measured within the column. Footnotes stay across the prose width.

Other sections have their own fields; the [schema](../schema/theme.v1.schema.json) lists them all.

## Templates

Title slots and header/footer slots hold text with placeholders. Kyber fills them from the document's metadata and the page.

### Placeholders

`{title}`, `{subtitle}`, `{author}`, `{date}`, `{abstract}`, `{section}`, `{subsection}`, `{page}`, `{pages}`, and `{meta.key}`.

- Write `{{` and `}}` for literal braces.
- Unknown placeholders and unbalanced braces are errors.
- Values are inserted as text. They are never read as layout commands or expressions.
- Title slots may not use `{section}`, `{subsection}`, `{page}`, or `{pages}`.
- Headers and footers may not use `{abstract}`.
- A slot's value is missing when any placeholder in it has no value or only spaces.
- `{author}` joins several authors with commas. `{page}` is the page number, counted from 1 including the title page, and `{pages}` the number of pages. `{section}` and `{subsection}` are explained under [header and footer bands](#header-and-footer-bands).
- `{meta.key}` is the entry `key` of the front matter `meta` map. Keys use letters, digits, `-`, and `_`. Any key is accepted here, since only the document knows its keys; a key the document lacks is a missing value.

A line break in slot text starts a new line, so `"Studio\nMain Street 1"` sets two lines. A `meta` value that is a list starts a new line with each entry after the first; text before the placeholder joins the first entry and text after it the last. Empty lines are dropped. Within a value, line breaks follow the rules of the slot kind below.

### Title slots

`title-block` has an ordered `slots` array, and each group of `title-page` has one. A slot is either text or an image:

| Field | Meaning |
| --- | --- |
| `text` | Slot text with placeholders. Use this or `image`, not both. |
| `image` | A name from the theme `images` map. |
| `width` | Image width. Required for image slots, an error on text slots. It may not exceed the prose width in the title block, or the group width on the title page. |
| `style` | One of `title`, `subtitle`, `author`, `date`, `abstract-heading`, `abstract`, `body`. Default `body`. |
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
| `style` | One of the title slot styles. Default: the `header` or `footer` style. |
| `required` | Default `false`. |
| `space-before` | Space above the slot's first line, default `0pt`. `em` refers to the slot style size. |

The header's baseline is `page.header-offset` above the text area and the footer's `page.footer-offset` below it. Groups are anchored to that baseline: a top anchor puts the group's first baseline `y` below it, a bottom anchor its last baseline `y` above it, and a middle anchor centers the group's baselines `y` below it. So a multi-line footer anchored at the top grows down toward the page edge, and a header anchored at the bottom grows up. Lines follow each other at the style's line height plus each slot's `space-before`. Every line of slot text is one line: it never wraps, and line breaks in values are spaces. A line wider than its group and lines of two groups that overlap are errors naming the band or group and the page, since nothing is clipped. An optional slot without a value is left out.

The default theme's left, center, and right band slots are groups as wide as the frame, aligned left, center, or right. Groups do not swap on even pages, so use the `odd` and `even` variants for bands that change sides.

`{section}` is the first level 1 heading that starts on the page, otherwise the last level 1 heading on an earlier page. `{subsection}` is the first level 2 heading on the page after that section, otherwise the last level 2 heading before the page, unless a level 1 heading followed it. Both include the heading number. On pages before the first heading they have no value.

### Missing values

A slot marked `required` whose value is missing is an error. Title slots are checked for the title layout in use as soon as the document's metadata is known, by `kyber check` and `kyber render`. Header and footer slots need the page, so `kyber render` checks them after layout and names the page. Optional slots with no value are omitted along with their spacing.

## Page variants

`pages` has the variants `title`, `first`, `odd`, `even`, and `body`. Only `body` is required. Which variant applies to a page:

| Page | Order tried |
| --- | --- |
| Title page | `title`, then `body`. |
| First body page | `first`, then `odd` or `even` by parity, then `body`. |
| Other body pages | `odd` or `even` by parity, then `body`. |

A null variant is skipped. Parity follows the physical page index in the PDF, counted from 1 and including the title page. The displayed page number `{page}` is the same number, so odd numbers are on odd pages, the right-hand pages in duplex printing. The first body page is the first page without a title page and the second page with one.

The inner margin sits on the left of odd pages and, unless `page.margins.mirror` is `false`, on the right of even pages.

## Labels

`labels` has an `en` and a `de` set with the keys `figure`, `table`, `section`, `page`, `contents`, `abstract`, `references`, `continued`. The set for `document.lang` is used.

| Key | Used for |
| --- | --- |
| `figure`, `table` | Captions, as in "Figure 1: ", and references to figures and tables, as in "Figure 1". |
| `section` | References to numbered headings, as in "Section 2.1". |
| `page` | Page references, as in "page 7". |
| `contents` | The table of contents heading. |
| `abstract` | The heading above an `abstract` title slot. |
| `references` | The bibliography heading, also in the table of contents, bookmarks, and running headers. |
| `continued` | The continuation of a footnote on the next page. |

A reference joins the label and the number with a no-break space. `captions.separator` is the text between the label and the caption, as in "Figure 1: ".

## Table of contents

With `document.toc` the body starts, after the title block, with the `contents` label in the `toc-heading` style and one entry per heading down to `document.toc-depth`, in the `toc-entry` style.

| Key | Effect |
| --- | --- |
| `toc.level-indent` | Indent per heading level below the first. Lines after an entry's first are indented by one level more. |
| `toc.leader` | Whether a row of dots leads from the entry text to its page number. |

The page number is right aligned at the edge of the text area, in a column at least three digits wide. Leader dots sit on a grid shared by all entries, so they line up. Each entry links to its heading.

## Bibliography

A document that cites sets a bibliography: the `references` label as an unnumbered heading in the `heading-1` style, then one entry per cited work in the `bibliography` style. `document.citation-style` chooses `author-date` or `numeric`; front matter can override it.

| Key | Effect |
| --- | --- |
| `bibliography.hanging-indent` | Indent of an entry's lines after its first. |
| `bibliography.entry-spacing` | Minimum space between entries. The style's own `space-before` and `space-after` also apply around each entry, and the larger space wins. |

Links in entries use `inline.link`, and so do citations, which link to their entry. Entry content and ordering are described in [authoring](AUTHORING.md#citations-and-bibliography).

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
