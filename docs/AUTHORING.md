# Authoring documents

A Kyber document is a Markdown file with optional YAML front matter. Front matter holds metadata and a small set of design settings. Everything else about the design comes from a [theme](THEMES.md).

`kyber render` turns a document into a PDF. Not every construct renders yet; see [Markdown content](#markdown-content) and [layout directives](#layout-directives).

## Front matter

Front matter is YAML between `---` lines at the very start of the file. It ends at the next line that is `---` or `...`. A file that does not start with `---` has no front matter.

```yaml
---
title: On quiet typography
author: [Ada Example, Bo Sample]
date: 2026-03-01
lang: en
toc: true
margins: 25mm
fonts:
  body: Libertinus Serif
---
```

YAML 1.2 rules apply: `yes`, `no`, `on`, and `off` are strings, not booleans. Use `true` and `false`. Unknown keys are errors. `null` is rejected, except for the `italic`, `bold`, and `bold-italic` faces in `font-files`, which follow the theme rule for font faces. To leave a setting alone, omit it.

Front matter cannot redefine templates (title slots, headers, footers). That belongs to the theme.

### Metadata

Metadata is plain text. A number or boolean is read as its text, so `date: 2024` needs no quotes; in `meta` and `--set`, a decimal such as `1.50` reads as `1.5`, so quote it to keep the zero. It is inserted into templates as text only: `*word*` stays two asterisks and a word. In `abstract`, a blank line starts a new paragraph.

| Key | Type | Effect |
| --- | --- | --- |
| `title` | string | Value of `{title}`. |
| `subtitle` | string | Value of `{subtitle}`. |
| `author` | string or list of strings | Value of `{author}`. |
| `date` | string | Value of `{date}`. |
| `abstract` | string | Value of `{abstract}`. |
| `meta` | map of strings or lists of strings | Value of `{meta.key}` for each `key`. |

Metadata fills the title block at the start of the first page, or the separate title page with `title-page: true`. See [document structure](#document-structure).

`meta` holds any other values a theme's slots use, such as an offer number or an address. Keys use letters, digits, `-`, and `_`. A list puts each entry on its own line; blank entries are skipped. Which keys a theme reads is up to the theme; a key no slot uses is ignored, and a key a slot needs but the document lacks leaves that slot out, or is an error if the theme marks the slot as required.

```yaml
meta:
  offer: "2026-117"
  client:
    - Northwind Instruments GmbH
    - Hafenstraße 12
    - 20457 Hamburg
```

On the command line, `--set meta.offer=2026-118` replaces one entry and keeps the others.

### Files

Paths are relative to the document.

| Key | Type | Effect |
| --- | --- | --- |
| `theme` | path | The theme file to use. `--theme` overrides it. |
| `bibliography` | path | BibTeX file for citations, see [citations](#citations-and-bibliography). |
| `font-files` | map | Adds font families, see below. |

`font-files` maps a family name to files, in the same shape as theme `fonts`, including other weights and faces of collections (see [fonts](THEMES.md#fonts-and-images)):

```yaml
font-files:
  My Serif:
    regular: fonts/MySerif-Regular.otf
    italic: fonts/MySerif-Italic.otf
    500: fonts/MySerif-Medium.otf
```

Use the family name in `fonts.body`, `fonts.heading`, or `fonts.mono` to select it. A family in `font-files` wins over an installed family with the same name. A name that neither the theme nor `font-files` defines, such as `fonts: {body: Avenir Next}`, is looked up among the fonts installed on the machine.

### Settings

Each setting writes one property of the resolved theme.

| Key | Values | Writes |
| --- | --- | --- |
| `lang` | `en`, `de` | `document.lang` |
| `title-page` | `true`, `false` | `document.title-page` |
| `toc` | `true`, `false` | `document.toc` |
| `duplex` | `true`, `false` | `document.duplex` |
| `numbered-headings` | `true`, `false` | `document.numbered-headings` |
| `numbering-depth` | 1 to 6 | `document.numbering-depth` |
| `toc-depth` | 1 to 6 | `document.toc-depth` |
| `citation-style` | `author-date`, `numeric` | `document.citation-style` |
| `page-size` | `a4`, `a5`, `b5`, `letter`, `legal`, or `{width, height}` | `page.size` |
| `margins` | one length, or `{top, bottom, inner, outer}` | `page.margins.*` |
| `column-gap` | length | `page.column-gap` |
| `font-size` | length | `styles.body.size` |
| `line-height` | number from 0.8 to 3 | `styles.body.line-height` |
| `paragraph-spacing` | length | `styles.body.space-after` |
| `fonts.body`, `fonts.heading`, `fonts.mono` | font family name | `tokens.fonts.body`, `tokens.fonts.heading`, `tokens.fonts.mono` |

Notes:

- A single `margins` length sets all four sides. A map sets only the sides it lists.
- Lengths use `pt`, `mm`, `cm`, `in`, or `em`. See [measurements](THEMES.md#measurements). `font-size` must be an absolute length because `em` sizes refer to it.
- `fonts.*` changes the font tokens. A theme only follows them if its styles reference `$fonts.body`, `$fonts.heading`, and `$fonts.mono`. The bundled theme does. A font setting that no style references prints a `warning:` line and has no effect.
- Settings are checked like theme values. A font family must exist in the theme, in `font-files`, or among the installed fonts, and must have the faces the styles request. A missing installed family, weight, or style is an error naming what was searched for. A font whose license restricts embedding is embedded with a warning.

## Command line

```
kyber check <doc.md> [--theme PATH] [--set KEY=VALUE]... [--print-config]
kyber render <doc.md> [-o PATH]
```

`check` does everything `render` does except write the PDF, so it reports the same problems, including a word wider than its line, an oversized table row or keep group, and a layout that does not settle. `--print-config` prints the resolved configuration as JSON instead and does not lay out.

`render` also writes the PDF. `-o PATH` is relative to the working directory; without it the PDF goes next to the document with a `.pdf` extension. If any diagnostic is reported, no PDF is written.

`--theme PATH` selects a theme and overrides the `theme` key in front matter. The path is relative to the working directory.

`--set KEY=VALUE` overrides a front matter setting. It uses the same keys, with dots for nesting. It can be repeated.

```
kyber check paper.md --set toc=true --set margins.top=2cm --set 'author=[A, B]'
```

- The value is parsed as YAML, so `--set date=2024` is a number and is rejected. Write `--set 'date="2024"'`.
- `theme` cannot be set this way. Use `--theme`.
- Paths in overrides are relative to the working directory.
- `--set` wins over front matter.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | One or more diagnostics. |
| 2 | Usage error, such as a missing argument. |

Diagnostics go to standard error, one per line:

```
error: <source>[:<line>:<column>]: [<property>: ]<message>
```

The source is the document, the theme file, "bundled default theme", or "command-line override". The property is a dotted path such as `styles.body.size`. For example:

```
error: theme.json: styles.body.size: length "12px" has unsupported unit "px" (use pt, mm, cm, in, or em)
```

## Markdown content

The body is CommonMark. These constructs render:

- Paragraphs and headings (`#` to `######`, or underlined).
- Bulleted and numbered lists, nested to any depth. Numbered lists keep their start number.
- Block quotations.
- Fenced and indented code blocks. Code is never wrapped: a code line wider than the text area is an error naming its line. Tabs become four spaces.
- `*emphasis*`, `**strong**`, `` `code` ``, links, and hard line breaks (a trailing backslash or two trailing spaces).
- [Images](#images-and-captions) with captions.
- [Tables](#tables) with captions.
- [Footnotes](#footnotes) and the [layout directives](#layout-directives) for columns, full-width blocks, keep groups, and page breaks.
- [Labels and cross-references](#labels-and-cross-references) to headings, figures, and tables.
- [Custom styles](#custom-styles) from the theme on headings, paragraphs, and lists.

Emphasis switches between upright and italic, so it is upright inside an italic quotation. Links are clickable and use the theme's link color.

A paragraph that follows another paragraph starts with the body style's `first-line-indent`. The first paragraph after a heading, list, quotation, code block, or a change between one and two columns does not. Keep groups do not interrupt that sequence.

Line breaks are chosen for each whole paragraph. Words are hyphenated by the rules of `lang` where the block style allows it, which the default theme does for body text, abstracts, quotations, lists, and footnotes but not for headings. Inline code and link text that spells out its URL are never hyphenated. A word also breaks after a hyphen that joins two words, as in "e-mail".

The following are reported as errors with their line and column, and no PDF is written:

- Not supported: thematic breaks (`---`), strikethrough, task lists, and raw HTML.

A character the selected font has no glyph for is an error, as is a word wider than the line even after hyphenation.

## Pages

Page breaks are chosen for the whole document. A heading always stays on the page of the text that follows it, with at least two lines of a paragraph after it. A paragraph's first or last line is never left alone at the bottom or top of a page, and neither is a table's first or last row unless two rows are too tall to share a page. A page avoids ending after a hyphenated line or after a sentence ending in a colon that introduces a list or code block. A continued footnote avoids leaving a single line on either page. The space between blocks may grow a little so that page bottoms line up; a page that cannot be filled that way runs short. The last page and a page before an explicit page break may be as short as needed.

Content that must stay on one page but is taller than the text area is an error naming its line: a keep group, or a heading with the start of its text. Nothing is clipped or dropped.

A theme may set prose narrower than the text area and let tables, figures, and code blocks use the whole width, see [margins and text width](THEMES.md#margins-and-text-width). Widths named below for text then mean the prose width.

## Images and captions

An image stands alone in its paragraph. Its description becomes the caption:

```markdown
![Pages typeset per year, 2021 to 2025.](figures/chart.svg)
```

- PNG, JPEG, and SVG files are supported. GIF, WebP, and other formats are errors, as is a file whose content does not match its extension.
- The path is relative to the Markdown file, wherever Kyber is run from. Put a path with spaces in angle brackets: `![Caption](<my chart.png>)`.
- Only local files are read. URLs, including `data:` URLs, are errors. An SVG file may embed images as data URLs but cannot refer to other files.
- The caption is numbered and labelled in the document language, as in "Figure 1: Pages typeset per year". It may use emphasis, strong text, and inline code. An image with an empty description, `![](file.png)`, has no caption and no number.
- A caption cannot hold a footnote, because CommonMark does not read `[^note]` inside an image description. Reference the note from the text next to the image.
- Image titles, `![Caption](file.png "Title")`, are errors. Write the caption as the description.
- A label right after the image, `![Caption](file.png){#fig:chart}`, lets the text refer to the figure, see [labels](#labels-and-cross-references). Only a figure with a caption can have one.

An image keeps its place in the text. There are no floats and text never wraps around an image. The image is centered and its caption follows below it. The two stay together: when they do not fit in the rest of a page or column, both move to the next column or page, and the page or column they leave ends short.

A PNG or JPEG with a stored density (PNG `pHYs` in meters, JPEG JFIF, else JPEG EXIF resolution) appears at its pixels divided by that density, so a 144 dpi screenshot is half as wide as its pixel count in points. Without a density, raster images count one pixel as one point (72 per inch). Aspect-only JFIF values do not count. SVG images use 96 pixels per inch. An image appears at that natural size unless it is too large. Then it shrinks, keeping its proportions, to the width of the text area or, inside `columns`, of the column. It also shrinks until it fits the height of a page together with its caption and a heading directly above it. Images never grow beyond their natural size. Put an image in a `full-width` block to give it the whole text width inside a column section.

Errors name the image's line and file: a missing or unreadable file, a malformed or unsupported image, and an image whose caption fills the whole page. An image inside a heading, a link, or a footnote is an error, and so is text in the same paragraph as an image. A paragraph that starts like an image but is not valid image syntax is an error too, rather than being printed as text.

SVG text uses the bundled Libertinus fonts, never the fonts installed on the machine, so an SVG renders the same everywhere.

## Tables

Tables are pipe tables as on GitHub, or [list tables](#list-tables) for cells with lists and spans. The second line of a pipe table sets each column's alignment: `:--` left, `:-:` centered, `--:` right, and `---` the alignment of the theme's cell style. A paragraph directly after the table that starts with a colon and a space is its caption:

```markdown
| Year | Event               | Pages |
|-----:|:--------------------|------:|
| 1455 | The *42-line Bible* | 1 286 |
| 1978 | TeX                 |   700 |

: Two events in the history of typesetting.
```

- Cells hold text with emphasis, strong text, inline code, links, and footnote references. Write `\|` for a pipe inside a cell, also inside code.
- A row with fewer cells than the header gets empty cells. A row with more cells is an error.
- Leave a blank line between the table and its caption, or the caption line becomes a row; that is reported.
- The caption is numbered and labelled in the document language, as in "Table 1: Two events in the history of typesetting.", and set above the table, as wide as the table and starting at its left edge. Over a table narrower than a third of the text width the caption runs on to the right edge of the text instead. Tables are numbered apart from figures. A table without a caption has no number.
- A caption must directly follow its table, and a table has at most one. A caption cannot hold footnotes. A label such as `{#tbl:events}` at its end lets the text refer to the table, see [labels](#labels-and-cross-references).

Column widths come from the cell text. A table whose cells all fit on one line keeps that natural width and is centered. A wider table fills the text width, or the column width inside `columns`: columns of short entries stay on one line, and columns of longer text share the rest equally and wrap. A word too wide even when every column is at its narrowest is an error at its cell.

A table can run over several pages and columns. Pages and columns break only between rows, never inside a row. The header row is repeated at the top of each page or column the table continues in, and the caption stays with the start of the table. A row is as tall as its tallest cell, and a row that does not fit on a page together with the repeated header is an error at the row. Notes referenced in a cell go at the foot of that row's page.

A table inside `full-width` spans the text area; the columns above it are balanced and resume below it. These are errors: an image in a cell, a footnote reference in the header row (the header repeats, so the note would have no single page), and a table inside a footnote. Pipe tables cannot merge cells; list tables can span columns.

### List tables

A `::: table` directive holds a list table: a list with one item per row, and in each row a nested list with one item per cell. The first row is the header. Cells hold paragraphs and lists, so an item can have a title line with a list of details below it:

```markdown
::: table {align="left right" widths="* auto"}
- - Item
  - Amount
- {.group}
  - {span=2} Theme and design
- - Theme in the corporate design

    {.detail}
    - Tokens for the corporate colours and typefaces
    - Cover and running footers
  - 2 280.00
- {.total}
  - {.sum-label} Total
  - 2 280.00
:::

: Estimated costs {#tbl:costs}
```

- `align` lists `left`, `center`, or `right` for each column, and `widths` lists `*` or `auto`. Both are optional, separated by spaces, and need one entry per column. Without `widths` every column is `auto`, which fits its content as for pipe tables. A `*` column takes the width the `auto` columns leave, shared equally among `*` columns, so a table with one fills its frame.
- `{span=n}` at the start of a cell makes it span `n` columns. The cells of every row must span as many columns as the header's; otherwise the row is an error. Rows cannot span.
- `{.name}` at the start of a row item styles all its cells, and at the start of a cell item, alone or with a span as in `{.name span=2}`, that cell. A cell's own style wins over its row's, and the column's alignment applies only to cells without a style of their own. Styles for rows and cells are paragraph styles; one with `rule-below` changes the rule below its row, see [themes](THEMES.md#custom-styles).
- The attributes may stand alone on the item's line, with the cells or text on the lines below.
- A caption, label, repeated header, rows that never split, and the errors for rows too tall work as for pipe tables. A cell spanning columns widens them where its text needs it.
- These are errors with their location: text in a row item outside its cells, a cell holding anything but paragraphs and lists, other attributes, and content in the directive besides the list. `align` or `widths` with the wrong number of entries is an error at the directive.

## Footnotes

Footnotes use the common Markdown extension syntax. A reference is `[^label]` in the text; the definition is `[^label]:` at the start of a line, anywhere in the document. Lines indented by four spaces continue the definition, so a note can hold several paragraphs.

```markdown
Kyber weighs all breaks together.[^weigh]

[^weigh]: The search looks at most one page ahead.

    A second paragraph of the same note.
```

Notes are numbered 1, 2, 3 in the order of their references. The number appears raised after the reference and at the start of the note. Notes are set at the foot of the page that holds their reference, below a short rule. A note too long for that page continues at the foot of the next one, under its number and "(continued)" ("(Fortsetzung)" in German). Labels are matched without regard to case. Lists and code in a note are set smaller by the same ratio as the note text.

These are errors:

- A reference without a definition, or a definition that is never referenced.
- A second reference to the same note. Each note is referenced once.
- Two definitions with the same label.
- A reference inside a footnote.
- A heading inside a footnote.

## Columns

Text in a `columns` section is set in two columns of equal width with `column-gap` between them. It runs down the first column, then down the second, then on to the next page if the section is longer than the room left on the page. Outside a section the text runs across the whole text area.

When a section ends, its last columns are balanced: the column break goes where both columns are most even, and the next block starts below the taller column. A `full-width` block inside a section does the same before it, runs across the text area, and the columns resume below it. Changing between one and two columns never starts a new page by itself. At least `column-gap` separates the columns from the full-width text above and below them.

Columns follow the same rules as pages. A heading stays at the top of its text, a `keep` group stays in one column, a table breaks between rows with its header repeated, and a paragraph's first or last line is not left alone at the foot or top of a column if a break one line less even avoids it. A keep group taller than a column is an error at its directive. So is a code line or word wider than a column, at its line. Footnotes referenced in either column go to the shared note area at the foot of the page, in reference order.

`page-break` inside a section ends the page. The columns above it are balanced, and the section continues in two columns on the next page.

## Layout directives

Directives are fenced containers that use three or more colons. They express layout intent only. There are no coordinates and no commands beyond the ones below.

```
::: columns
Two equal columns, text flows down the first then the second.

::: full-width
A block across the whole text area. Preceding columns are balanced first.
:::

More two-column text.
:::

::: keep
Content kept on one page.
:::

::: page-break
```

| Directive | Effect |
| --- | --- |
| `columns` | Lays the content out in two equal columns. |
| `full-width` | A block across the whole text area inside `columns`. The columns before it are balanced first. |
| `keep` | Keeps the content together on one page. If it cannot fit, that is reported, not silently split. |
| `page-break` | Starts a new page. |
| `table` | A list table, see [list tables](#list-tables). |
| `bibliography` | Places the bibliography here instead of at the end, see [citations](#citations-and-bibliography). |

Rules:

- A directive line starts at the beginning of a line, outside lists, quotations, and footnotes. It needs no blank line around it.
- An opening line is colons, a space, and a directive name. Only `table` takes attributes after its name.
- A line of only colons closes the innermost open container.
- `page-break` and `bibliography` stand alone. They have no body and no closing line.
- `columns` cannot nest.
- `full-width` is only allowed directly inside `columns`.
- `page-break` and `bibliography` are not allowed inside `keep`. Inside `columns` a page break ends the page, see [columns](#columns).
- `bibliography` appears at most once.
- `columns` is not allowed inside `keep`. Put keep groups inside the columns instead.
- Directive lines inside fenced code blocks are code, not directives.
- Unknown names, unclosed fences, and unmatched closing lines are errors that name the source line. So is a directive line between the items of a list or inside another block.
- Several page breaks in a row start one new page. A page break before all content or after it has no effect.
- Outside `columns` the layout is one column.

Embedded HTML is not rendered.

An `@key` without a `sec:`, `fig:`, or `tbl:` prefix is a citation, see [citations](#citations-and-bibliography).

## Document structure

### Title block and title page

A document with a `title`, `subtitle`, `author`, `date`, or `abstract` starts with a title block: the theme's title slots filled with the metadata, followed by some space. Without any of them there is no title block. With `title-page: true` the title, authors, date, and abstract fill a first page of their own instead, and the text starts on the next page.

A slot whose value is missing is left out with the space above it. The theme marks some slots as required; the default theme requires `title`, and a missing required value is an error that names the slot, from `kyber check` and `kyber render`. The abstract is set under the heading "Abstract" ("Zusammenfassung" in German).

A theme may place several groups of slots on the title page, such as an address block at the top right and the title halfway down. [`samples/offer.md`](../samples/offer.md) with [its theme](../samples/themes/business.json) shows a cover of that kind.

### Numbered headings

Headings are numbered 1, 1.1, 1.1.1 down to `numbering-depth` (3 in the default theme). Deeper headings have no number. A number is followed by one space unless the theme sets `number-gap` on the heading style, which sets the number hanging. `numbered-headings: false` turns numbering off.

### Table of contents

`toc: true` sets a table of contents after the title block, or at the top of the first page after a title page. It lists headings down to `toc-depth` (2 in the default theme) with their numbers and pages, and each entry is a link to its heading. To start the text on a new page after the contents, begin the body with `::: page-break`.

`duplex: true` prepares a document for printing on both sides: the text starts on an odd page, the right-hand page. A blank page follows the title page, and the contents end their page and are followed by a blank page if needed. Blank pages have no header or footer and count in the page numbers.

### Page numbers and running headers

Pages are numbered from 1, counting the title page, so the number on a page is the one a PDF viewer shows. Themes can also show the total, as in "Page 2|4". The default theme puts the page number at the foot of every page except the title page, and the current section in the header of every page after the first. The section shown is the first numbered top-level heading that starts on the page, or the last one before it. Themes change all of this, see [themes](THEMES.md#header-and-footer-bands).

Headings appear as bookmarks in the PDF, nested by level.

## Labels and cross-references

A label names a heading, a figure, or a table so the text can refer to it. It always starts with the kind of thing it names:

| Kind | Label |
| --- | --- |
| Heading | `# Results {#sec:results}` |
| Figure | `![Pages per year.](chart.svg){#fig:chart}` |
| Table | `: Events in typesetting. {#tbl:events}` after the table |

The name after the prefix uses letters, digits, `-`, and `_`. A reference writes the label after an at sign:

```markdown
@sec:results shows the totals, and @fig:chart the trend. The data is in
[@tbl:events], which starts on [@tbl:events, page].
```

This reads "Section 2 shows the totals, and Figure 1 the trend. The data is in Table 1, which starts on page 4." The words come from the theme's labels in the document language. A reference to a heading without a number shows the heading's text. `[@label, page]` shows the page the heading, figure, or table starts on. Every reference is a link to its target.

Numbers and page numbers are those of the final layout. Page references and the table of contents can move text to other pages; Kyber lays the document out again until all of them agree, usually in two passes, and reports an error naming the target if they do not settle.

Write `\@` for an at sign that should stay text before a prefix. An `@` right after a letter or digit, as in an e-mail address, is never a reference.

These are errors with their location:

- A reference to a label that is not defined, and a label defined twice.
- A label with the wrong prefix for what it names, or a name with other characters.
- A label on an image without a caption, or on a heading together with attributes other than one [style](#custom-styles).
- A reference in a heading or inside a link.
- Brackets that hold more than the reference or `, page`, as in `[@sec:a; @sec:b]`.

## Custom styles

A theme can define named styles in `custom-styles`, see [themes](THEMES.md#custom-styles). Apply one with `{.name}`:

```markdown
Scope {.eyebrow}

# What is included {#sec:scope .wide-heading}

{.checks}
- Layout of all chapters
- Two rounds of corrections

### 2. Agree on the design {.step}
```

- At the end of a heading, alone or together with its label, as in `{#sec:scope .wide-heading}`.
- At the end of a paragraph's last line.
- At the start of a row or cell item of a [list table](#list-tables).
- On a line of its own directly before a list, also inside a list item before a nested list. The line may follow the text of a paragraph without a blank line; then it styles the list, not the paragraph.

A style is for one kind of block: a style based on a heading style for headings, one based on `list` for lists, and any other for paragraphs and table rows and cells. Which kind a style is, and what it changes, is up to the theme. Common uses are a small spaced label kept with the heading below it, a list with check marks, and step headings whose typed number hangs in front of the text.

Write `\{.name}` for text that should keep the braces. These are errors with their location:

- A style the theme does not define, or one for another kind of block.
- `{.name}` on a line of its own that no list follows.
- `{.name}` alone on a list item or as the only content of a paragraph that is not on a line of its own.
- More than one style, or other attributes, on a heading.

## Citations and bibliography

Set `bibliography` to a BibTeX file, relative to the document. With `--set bibliography=refs.bib` the path is relative to the working directory. Cite a work with an at sign and its key:

```markdown
Paragraphs are broken as a whole [@knuthplass1981], and hyphenation
follows patterns [@liang1983, p. 37; @knuth1984]. @bringhurst2004 [pp. 25-26]
calls the result rhythm.
```

| Written | Author-date | Numeric |
| --- | --- | --- |
| `[@liang1983]` | (Liang 1983) | [2] |
| `[@liang1983; @knuth1984]` | (Liang 1983; Knuth 1984) | [2, 3] |
| `[@liang1983, p. 37]` | (Liang 1983, p. 37) | [2, p. 37] |
| `@liang1983` | Liang (1983) | Liang [2] |
| `@bringhurst2004 [pp. 25-26]` | Bringhurst (2004, pp. 25–26) | Bringhurst [4, pp. 25–26] |

`citation-style: author-date` or `numeric` in the front matter chooses the style; the default theme uses author-date. The words follow `lang`: "and" becomes "und", "p." becomes "S.".

### Citation syntax

- A key is letters, digits, and `_`, with `-`, `:`, `.`, `/`, or `+` between them. Keys that start with `sec:`, `fig:`, or `tbl:` are cross-references, never citations.
- A citation in brackets holds at least one `@key` and ends at the first `]`. Several keys are separated by `;`. The brackets may span lines. A bracket without any `@key` is text.
- Text before a key is its prefix: `[see @smith2024, p. 3; also @lee2022]` reads "(see Smith 2024, p. 3; also Lee et al. 2022)". Each work in a group takes its own prefix. In the numeric style the prefix stays inside the brackets: "[see 1, p. 3; also 2]". A prefix is plain text, and it is not part of the link. A narrative citation takes no prefix.
- A locator follows a key after a comma: `p.`, `pp.`, or `S.`, then a page or a range of two pages made of letters and digits, as in `p. 12`, `pp. 3-5`, `S. xiv`. The range gets an en dash. A no-break space keeps the label with the page. `pp.` is used for a range and `p.` for one page, whichever was written.
- Text after the locator is a suffix, as in Pandoc: `[@smith2024, p. 3, emphasis added]` reads "(Smith 2024, p. 3, emphasis added)". Text after the comma that does not start with a locator is all suffix: `[@smith2024, emphasis added]`. A suffix is printed as written, after the locator and a comma, and is part of the link. Whatever follows the key must start with a comma. The numeric style puts it inside the brackets: "[1, p. 3, emphasis added]".
- A narrative citation is `@key` where a word starts: at the start of a line, after a space, or after an opening bracket or quotation mark. A locator follows it in brackets after one space on the same line: `@key [p. 12]`, optionally with a suffix: `@key [p. 12, passim]`. Brackets that do not start with a locator stay text.
- An `@` after a letter or digit, as in an e-mail address, is no citation. Write `\@` for an at sign that starts a word and should stay text. In link text and inline code, `@` is always text.

### Author-date style

- A citation names the authors: one by family name, two as "Smith and Jones", three or more as "Smith et al.". Without authors, the editors stand in, then a report's institution, then the title.
- Two cited works with the same name and year get a letter after the year, in bibliography order: "Knuth 1984a", "Knuth 1984b".
- A web resource without a year shows "n.d." ("o. J." in German).
- The bibliography is sorted by the name that leads each entry, with umlauts sorted as "ae", "oe", "ue", then by year, title, and key.

### Numeric style

- Works are numbered in the order they are first cited, reading the text from the start. A citation in a footnote counts where the footnote is referenced.
- Numbers in a citation are sorted, and runs of three or more become a range: [1–3, 5]. With a locator, a prefix, or a suffix each work is listed apart, in the order written: [1, p. 12; 3].
- The bibliography lists the works in number order, each with its label, as in "[1]". The labels stand in a column as wide as the widest one, and the text of every entry starts after it, also on later lines. `bibliography.hanging-indent` does not apply.

### The bibliography

A document that cites gets a bibliography: an unnumbered top-level heading with the theme's `references` label ("References", "Literatur" in German), then one entry for each work cited, each listed once. Works in the BibTeX file that are not cited are left out. The heading is in the table of contents, the PDF bookmarks, and running headers like any other top-level heading.

The bibliography goes at the end of the document. `::: bibliography` on a line of its own places it there instead, for example before an appendix, or inside `::: columns` to set it in two columns. Without citations there is no bibliography, even with the directive.

Entries use the theme's `bibliography` style. Each entry's later lines hang by `bibliography.hanging-indent`, entries are `bibliography.entry-spacing` apart, and they break across pages and columns like paragraphs. Each work in a citation is a link to its own entry, so "(Smith 2024; Lee 2022)" has two links and "[1, 3]" has two. In a range such as "[1–3]" the first and last number link.

An entry reads the same in both styles:

1. The lead and the year: `Smith, Ada and Bob Jones (2024).` The first author's name is inverted. Editors get "(Ed.)" or "(Eds.)" ("Hrsg." in German).
2. The title, unless it already leads. Books, theses, and reports set it in italics.
3. The details of the entry type:
   - article: `*Journal* 12(3), 45–67.`
   - book: the edition, as in `2nd ed.` (`2. Aufl.`) for `edition = {2}`, other text as written; then `Address: Publisher.`
   - conference paper: `In: *Proceedings*, 45–67.`, then `Address: Publisher.`
   - thesis: `PhD thesis, School.` (`Master's thesis`, `Thesis`); a `type` field replaces the wording.
   - report: `Technical report 42, Institution, Address.`; the institution is left out when it leads.
   - web resource: the publisher or organization, and after the URL `Accessed 2024-05-05.` (`Abgerufen am`) from `urldate`.
4. The DOI as a `https://doi.org/` link, else the URL. Web resources show the URL first.

URLs and DOIs break only after a single `/`, without a hyphen, so one with a part wider than the line, which happens most in narrow columns, is an error at its entry. The same holds for link text in the document that spells out its URL.

### Entry types and fields

| Type | Required | Also used |
| --- | --- | --- |
| `article` | author, title, journal, year | volume, number, pages, doi, url |
| `book` | author or editor, title, publisher, year | edition, address, doi, url |
| `inproceedings`, `conference` | author, title, booktitle, year | pages, publisher, address, doi, url |
| `phdthesis`, `mastersthesis`, `thesis` | author, title, school (or institution), year | type, doi, url |
| `techreport`, `report` | title, institution, year | author, type, number, address, doi, url |
| `online`, `misc` | title, url | author, year, publisher (or organization), urldate |

Other fields are ignored, including the BibLaTeX fields `date`, `journaltitle`, and `location`; use `year`, `journal`, and `address`. Entry types and field names are not case-sensitive. `@comment` and `@preamble` are skipped. Names are separated by `and`; `and others` reads as "et al.", and a name in braces, such as `{World Health Organization}`, stays whole.

Text may use the LaTeX escapes for accents and umlauts (`{\"a}`, `\'e`, `` \`a ``, `\^o`), `{\ss}`, escaped `\&`, `\%`, `\$`, `\#`, `\_`, `--` and `---` for dashes, `~` for a no-break space, and braces, which are removed. `url` and `doi` are taken as written. Month names such as `jan` may stand without braces.

### Errors

These are errors, reported with their location:

- A citation without a `bibliography` setting, at the first citation.
- A key that is not in the bibliography, at the key.
- Brackets that start like a citation but cannot be read, such as text after a key without a comma in `[@key see below]`, and brackets that mix a citation with a cross-reference, as in `[@key; @sec:intro]`. Write them apart.
- A citation in a heading.
- A second `::: bibliography`, or one inside `keep`.
- A bibliography file that does not exist, by the `bibliography` setting.
- In the BibTeX file, at its line and column: syntax errors, unsupported entry types, keys defined twice, missing required fields, other LaTeX commands, `@string` macros, and `crossref`.
