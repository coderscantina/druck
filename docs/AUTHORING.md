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

Metadata is plain text. It is inserted into templates as text only.

| Key | Type | Effect |
| --- | --- | --- |
| `title` | string | Value of `{title}`. |
| `subtitle` | string | Value of `{subtitle}`. |
| `author` | string or list of strings | Value of `{author}`. |
| `date` | string | Value of `{date}`. |
| `abstract` | string | Value of `{abstract}`. |

### Files

Paths are relative to the document.

| Key | Type | Effect |
| --- | --- | --- |
| `theme` | path | The theme file to use. `--theme` overrides it. |
| `bibliography` | path | BibTeX file for citations. |
| `font-files` | map | Adds font families, see below. |

`font-files` maps a family name to files, in the same shape as theme `fonts`:

```yaml
font-files:
  My Serif:
    regular: fonts/MySerif-Regular.otf
    italic: fonts/MySerif-Italic.otf
```

Use the family name in `fonts.body`, `fonts.heading`, or `fonts.mono` to select it.

### Settings

Each setting writes one property of the resolved theme.

| Key | Values | Writes |
| --- | --- | --- |
| `lang` | `en`, `de` | `document.lang` |
| `title-page` | `true`, `false` | `document.title-page` |
| `toc` | `true`, `false` | `document.toc` |
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
- `fonts.*` changes the font tokens. A theme only follows them if its styles reference `$fonts.body`, `$fonts.heading`, and `$fonts.mono`. The bundled theme does.
- Settings are checked like theme values. A font family must exist in the theme or in `font-files`, and must have the faces the styles request.

## Command line

```
kyber check <doc.md> [--theme PATH] [--set KEY=VALUE]... [--print-config]
kyber render <doc.md> [-o PATH]
```

`check` loads the document, theme, and overrides, resolves the configuration, and reports problems. `--print-config` prints the resolved configuration as JSON.

`render` validates the same way, then parses the Markdown, lays it out, and writes the PDF. `-o PATH` is relative to the working directory; without it the PDF goes next to the document with a `.pdf` extension. If any diagnostic is reported, no PDF is written.

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
- [Footnotes](#footnotes) and the [layout directives](#layout-directives) for columns, full-width blocks, keep groups, and page breaks.

Emphasis switches between upright and italic, so it is upright inside an italic quotation. Links are clickable and use the theme's link color.

A paragraph that follows another paragraph starts with the body style's `first-line-indent`. The first paragraph after a heading, list, quotation, code block, or a change between one and two columns does not. Keep groups do not interrupt that sequence.

Line breaks are chosen for each whole paragraph. Words are hyphenated by the rules of `lang` where the block style allows it, which the default theme does for body text, abstracts, quotations, lists, and footnotes but not for headings. Inline code and link text that spells out its URL are never hyphenated. A word also breaks after a hyphen that joins two words, as in "e-mail".

The following are reported as errors with their line and column, and no PDF is written:

- Not supported yet: images and tables. They arrive in later milestones.
- Not supported: thematic breaks (`---`), strikethrough, task lists, and raw HTML.

A character the selected font has no glyph for is an error, as is a word wider than the line even after hyphenation. `kyber check` validates configuration only and does not read the Markdown body.

## Pages

Page breaks are chosen for the whole document. A heading always stays on the page of the text that follows it. A paragraph's first or last line is not left alone at the bottom or top of a page if a better break exists. The space between blocks may grow a little so that page bottoms line up; a page that cannot be filled that way runs short. The last page and a page before an explicit page break may be as short as needed.

Content that must stay on one page but is taller than the text area is an error naming its line: a keep group, or a heading with the start of its text. Nothing is clipped or dropped.

## Footnotes

Footnotes use the common Markdown extension syntax. A reference is `[^label]` in the text; the definition is `[^label]:` at the start of a line, anywhere in the document. Lines indented by four spaces continue the definition, so a note can hold several paragraphs.

```markdown
Kyber weighs all breaks together.[^weigh]

[^weigh]: The search looks at most one page ahead.

    A second paragraph of the same note.
```

Notes are numbered 1, 2, 3 in the order of their references. The number appears raised after the reference and at the start of the note. Notes are set at the foot of the page that holds their reference, below a short rule. A note too long for that page continues at the foot of the next one, under its number and "(continued)" ("(Fortsetzung)" in German). Labels are matched without regard to case.

These are errors:

- A reference without a definition, or a definition that is never referenced.
- A second reference to the same note. Each note is referenced once.
- Two definitions with the same label.
- A reference inside a footnote.

## Columns

Text in a `columns` section is set in two columns of equal width with `column-gap` between them. It runs down the first column, then down the second, then on to the next page if the section is longer than the room left on the page. Outside a section the text runs across the whole text area.

When a section ends, its last columns are balanced: the column break goes where both columns are most even, and the next block starts below the taller column. A `full-width` block inside a section does the same before it, runs across the text area, and the columns resume below it. Changing between one and two columns never starts a new page by itself. At least `column-gap` separates the columns from the full-width text above and below them.

Columns follow the same rules as pages. A heading stays at the top of its text, a `keep` group stays in one column, and a paragraph's first or last line is not left alone at the foot or top of a column if a break one line less even avoids it. A keep group taller than a column is an error at its directive. So is a code line or word wider than a column, at its line. Footnotes referenced in either column go to the shared note area at the foot of the page, in reference order.

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

Rules:

- A directive line starts at the beginning of a line, outside lists, quotations, and footnotes. It needs no blank line around it.
- An opening line is colons, a space, and a directive name.
- A line of only colons closes the innermost open container.
- `page-break` stands alone. It has no body and no closing line.
- `columns` cannot nest.
- `full-width` is only allowed directly inside `columns`.
- `page-break` is not allowed inside `keep`. Inside `columns` it ends the page, see [columns](#columns).
- `columns` is not allowed inside `keep`. Put keep groups inside the columns instead.
- Directive lines inside fenced code blocks are code, not directives.
- Unknown names, unclosed fences, and unmatched closing lines are errors that name the source line. So is a directive line between the items of a list or inside another block.
- Several page breaks in a row start one new page. A page break before all content or after it has no effect.
- Outside `columns` the layout is one column.

Embedded HTML is not rendered.

Syntax for captions, labels, cross-references, and citations (`[@key]`) is documented with the milestones that implement them.
